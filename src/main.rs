mod assets;
mod audio;
mod config;
mod engine;
mod normalize;
mod pipeline;
mod segment;
mod wire;
use anyhow::{Context, Result, ensure};
use axum::{Json, Router, extract::State, routing::get};
use config::Config;
use engine::Engine;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Instant};
use supertonic3_tts::Style;
use tokio::{
    io::BufReader,
    net::{TcpListener, TcpStream},
    sync::{Semaphore, watch},
    task::JoinSet,
};
fn onnx_version() -> String {
    // ORT owns this NUL-terminated version string for the process lifetime.
    unsafe {
        std::ffi::CStr::from_ptr(((*ort::sys::OrtGetApiBase()).GetVersionString)())
            .to_string_lossy()
            .into_owned()
    }
}
struct App {
    config: Config,
    model_load_seconds: f64,
    engines: Vec<Arc<Engine>>,
    voices: BTreeMap<String, Arc<Style>>,
    permits: Arc<Semaphore>,
    free: std::sync::Mutex<Vec<usize>>,
}
impl App {
    fn voices(&self) -> Vec<Value> {
        self.voices.keys().map(|id|json!({"name":id,"description":format!("Supertonic 3 {id}"),"languages":[self.config.language],"installed":true,
            "attribution":{"name":"Supertone / voice file owner","url":"https://huggingface.co/Supertone/supertonic-3"},"version":"3"})).collect()
    }
    fn health(&self) -> Value {
        json!({"status":"ok","engine":"supertonic3-tts","engine_version":"1.3.0","onnx_runtime_version":onnx_version(),"language":self.config.language,
        "german_normalization":self.config.german_normalization,"default_voice":self.config.voice,"voices":self.voices.keys().collect::<Vec<_>>(),"model_loaded":true,
        "sample_rate":44100,"execution_provider":"CPUExecutionProvider","threads":self.config.threads,
        "steps":self.config.steps,"speed":self.config.speed,"model_load_seconds":self.model_load_seconds,
        "ort_configured":self.config.ort_configured,"ort_thread_pool":self.config.ort_pool,"ort_inter_threads":self.config.ort_inter,"ort_spinning":self.config.ort_spin,"ort_execution_mode":if self.config.ort_parallel{"parallel"}else{"sequential"},
        "chunk_bytes":self.config.chunk_bytes,"optimized_datapath":self.config.optimized_datapath,"buffered_tcp":self.config.buffered_tcp,"streaming":self.config.streaming,"streaming_prefill_ms":self.config.streaming_prefill_ms,"concurrent_requests":self.config.concurrent,"model_revision":assets::REVISION})
    }
}
struct EngineLease {
    app: Arc<App>,
    index: usize,
    _permit: tokio::sync::OwnedSemaphorePermit,
}
impl Drop for EngineLease {
    fn drop(&mut self) {
        self.app.free.lock().unwrap().push(self.index);
    }
}
async fn synthesis(app: Arc<App>, data: &Value) -> Result<Vec<u8>> {
    let text = data["text"].as_str().context("missing text")?.to_owned();
    ensure!(
        !text.trim().is_empty() && text.len() <= 16384,
        "text is empty or too long"
    );
    ensure!(
        data["text_format"].as_str().is_none_or(|v| v == "text"),
        "only plain text supported"
    );
    let voice = data["voice"]["name"].as_str().unwrap_or(&app.config.voice);
    let style = app.voices.get(voice).context("unknown voice")?.clone();
    let lang = normalize::language(
        data["voice"]["language"]
            .as_str()
            .unwrap_or(&app.config.language),
    )
    .to_owned();
    ensure!(
        supertonic3_tts::is_valid_lang(&lang),
        "unsupported language"
    );
    let text = normalize::apply(&text, app.config.german_normalization, &lang).into_owned();
    let permit = app.permits.clone().acquire_owned().await?;
    let index = app.free.lock().unwrap().pop().context("no free engine")?;
    let lease = EngineLease {
        app: app.clone(),
        index,
        _permit: permit,
    };
    let runtime = tokio::runtime::Handle::current();
    // ONNX inference is synchronous inside the upstream async API. Keep it off Tokio's I/O workers.
    tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        let start = Instant::now();
        let audio = runtime.block_on(app.engines[lease.index].synthesize(
            &text,
            &style,
            app.config.speed,
            &lang,
            app.config.steps,
        ))?;
        ensure!(
            !audio.is_empty() && audio.iter().all(|v| v.is_finite()),
            "invalid engine audio"
        );
        tracing::info!(
            seconds = start.elapsed().as_secs_f64(),
            audio_seconds = audio.len() as f64 / 44100.0,
            steps = app.config.steps,
            "synthesis complete"
        );
        let pcm = audio
            .into_iter()
            .flat_map(|v| ((v.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes())
            .collect();
        drop(lease);
        Ok(pcm)
    })
    .await?
}
async fn connection(
    stream: TcpStream,
    app: Arc<App>,
    mut stop: watch::Receiver<bool>,
) -> Result<()> {
    if app.config.buffered_tcp {
        stream.set_nodelay(true)?;
    }
    let (reader, writer) = stream.into_split();
    let mut writer: Box<dyn tokio::io::AsyncWrite + Unpin + Send> = if app.config.buffered_tcp {
        Box::new(tokio::io::BufWriter::with_capacity(
            app.config.chunk_bytes + 256,
            writer,
        ))
    } else {
        Box::new(writer)
    };
    let mut reader = BufReader::new(reader);
    loop {
        if *stop.borrow() {
            return Ok(());
        }
        let event = tokio::select! {
            _=stop.changed()=>return Ok(()),
            result=tokio::time::timeout(std::time::Duration::from_secs(300),wire::read(&mut reader))=>result??,
        };
        let Some(event) = event else { return Ok(()) };
        match event["type"].as_str().unwrap_or("") {
            "describe"=>wire::write(&mut writer,"info",json!({"tts":[{
                "name":"supertonic3","description":"Local Supertonic 3 TTS","version":"1.3.0","installed":true,
                "attribution":{"name":"Supertone","url":"https://huggingface.co/Supertone/supertonic-3"},
                "voices":app.voices(),"supports_synthesize_streaming":true}]}),&[]).await?,
            "synthesize-start"=>{
                if let Err(error)=pipeline::respond(&mut reader,&mut writer,app.clone(),&event["data"],true,&mut stop).await {
                    tracing::warn!(%error,"streaming request rejected");
                    wire::write(&mut writer,"error",json!({"code":"synthesis-failed","text":error.to_string()}),&[]).await?;
                    return Ok(())
                }
            },
            "synthesize" if app.config.streaming || app.config.optimized_datapath=>{
                if let Err(error)=pipeline::respond(&mut reader,&mut writer,app.clone(),&event["data"],false,&mut stop).await {
                    tracing::warn!(%error,"synthesis request rejected");
                    wire::write(&mut writer,"error",json!({"code":"synthesis-failed","text":error.to_string()}),&[]).await?;
                }
            },
            "synthesize"=>match synthesis(app.clone(),&event["data"]).await {
                Ok(pcm)=>{
                    let format=json!({"rate":44100,"width":2,"channels":1});
                    wire::write(&mut writer,"audio-start",format.clone(),&[]).await?;
                    for chunk in pcm.chunks(app.config.chunk_bytes) {wire::write(&mut writer,"audio-chunk",format.clone(),chunk).await?;}
                    wire::write(&mut writer,"audio-stop",json!({}),&[]).await?;
                }
                Err(error)=>{
                    tracing::warn!(%error,"synthesis request rejected");
                    wire::write(&mut writer,"error",json!({"code":"synthesis-failed","text":error.to_string()}),&[]).await?;
                }
            },
            _=>{}
        }
    }
}
async fn signal() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("SIGTERM handler");
    tokio::select! {_=term.recv()=>{},_=tokio::signal::ctrl_c()=>{}}
    tracing::info!("shutdown requested");
}
async fn serve(mut c: Config, mut receiver: watch::Receiver<bool>) -> Result<()> {
    tokio::select! {result=assets::prepare(&c)=>result?,_=receiver.changed()=>return Ok(())};
    let voices: BTreeMap<String, Arc<Style>> = assets::scan(&c.voices)?
        .into_iter()
        .map(|(id, style)| (id, Arc::new(style)))
        .collect();
    if !voices.contains_key(&c.voice) {
        let fallback = voices.keys().next().context("no valid voice")?.clone();
        tracing::warn!(requested=%c.voice,%fallback,"default voice unavailable; using valid fallback");
        c.voice = fallback;
    }
    let pool = ort::environment::GlobalThreadPoolOptions::default()
        .with_intra_threads(c.threads)?
        .with_inter_threads(c.ort_inter)?
        .with_spin_control(c.ort_spin)?;
    let mut environment = ort::init().with_name("supertonic3").with_telemetry(false);
    if c.ort_pool == "global" {
        environment = environment.with_global_thread_pool(pool);
    }
    ensure!(environment.commit(), "ORT already initialized");
    tracing::info!(version=%onnx_version(),build=ort::info(),"ONNX Runtime initialized");
    let load_start = Instant::now();
    let mut engines = Vec::new();
    for _ in 0..c.concurrent {
        let engine = Engine::load(&c).await?;
        tracing::info!(backend = "CPU", "model loaded");
        engines.push(Arc::new(engine));
    }
    let model_load_seconds = load_start.elapsed().as_secs_f64();
    let wyoming = TcpListener::bind(("0.0.0.0", c.wyoming)).await?;
    let http = TcpListener::bind(("0.0.0.0", c.http)).await?;
    let n = c.concurrent;
    let app = Arc::new(App {
        config: c,
        model_load_seconds,
        engines,
        voices,
        permits: Arc::new(Semaphore::new(n)),
        free: std::sync::Mutex::new((0..n).collect()),
    });
    let router = Router::new()
        .route(
            "/health",
            get(|State(a): State<Arc<App>>| async move { Json(a.health()) }),
        )
        .route(
            "/v1/audio/voices",
            get(|State(a): State<Arc<App>>| async move { Json(json!({"voices":a.voices()})) }),
        )
        .with_state(app.clone());
    let mut http_stop = receiver.clone();
    let http_task = tokio::spawn(async move {
        axum::serve(http, router)
            .with_graceful_shutdown(async move {
                let _ = http_stop.changed().await;
            })
            .await
    });
    let mut clients = JoinSet::new();
    tracing::info!(
        wyoming_port = app.config.wyoming,
        http_port = app.config.http,
        voices = app.voices.len(),
        "server ready"
    );
    loop {
        tokio::select! {
            _=receiver.changed()=>break,
            accepted=wyoming.accept()=>{
                let (stream,_)=accepted?;
                let a=app.clone();let stop=receiver.clone();
                clients.spawn(async move{if let Err(error)=connection(stream,a,stop).await {tracing::warn!(%error,"client disconnected");}});
            },
            Some(result)=clients.join_next(),if !clients.is_empty()=>{if let Err(error)=result{tracing::warn!(%error,"client task failed");}}
        }
    }
    tokio::time::timeout(std::time::Duration::from_secs(25), async {
        while clients.join_next().await.is_some() {}
        let _ = http_task.await;
    })
    .await
    .ok();
    clients.abort_all();
    tracing::info!("shutdown complete");
    Ok(())
}
#[tokio::main(worker_threads = 2)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let c = Config::load()?;
    if std::env::args().any(|a| a == "--healthcheck") {
        let response = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{}/health", c.http))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await?
            .error_for_status()?;
        let v: Value = serde_json::from_str(&response.text().await?)?;
        ensure!(
            v["model_loaded"] == true && v["status"] == "ok",
            "unhealthy"
        );
        return Ok(());
    }
    let (sender, receiver) = watch::channel(false);
    tokio::spawn(async move {
        signal().await;
        let _ = sender.send(true);
    });
    serve(c, receiver).await
}
