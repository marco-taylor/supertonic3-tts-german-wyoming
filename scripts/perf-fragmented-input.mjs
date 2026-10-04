import net from 'node:net';
import fs from 'node:fs';
const output=process.argv[2],wavPath=process.argv[3];
const text='Guten Morgen. Dies ist ein Test der deutschen Sprachausgabe mit Supertonic drei.';
const encode=(type,data={})=>Buffer.from(JSON.stringify({type,data})+'\n');
const events=[],payloads=[];let buffer=Buffer.alloc(0),failure;
const socket=net.createConnection({host:'127.0.0.1',port:Number(process.env.BENCH_WYOMING_PORT||10200)});
socket.setTimeout(20000,()=>socket.destroy(Error('fragmented-input timeout')));
const end=new Promise(resolve=>{socket.on('close',resolve);socket.on('error',e=>{failure=e.message})});
socket.on('data',incoming=>{
 buffer=Buffer.concat([buffer,incoming]);
 for(;;){
  const line=buffer.indexOf(10);if(line<0)break;
  const header=JSON.parse(buffer.subarray(0,line).toString()),dataLength=header.data_length||0,bytes=header.payload_length||0;
  if(buffer.length<line+1+dataLength+bytes)break;
  const data={...(header.data||{}),...(dataLength?JSON.parse(buffer.subarray(line+1,line+1+dataLength).toString()):{})};
  events.push(header.type);
  if(header.type==='audio-start'||header.type==='audio-chunk'){
   if(data.rate!==44100||data.width!==2||data.channels!==1)failure='invalid audio metadata';
  }
  if(header.type==='audio-chunk')payloads.push(Buffer.from(buffer.subarray(line+1+dataLength,line+1+dataLength+bytes)));
  if(header.type==='error')failure='server error: '+data.text;
  buffer=buffer.subarray(line+1+dataLength+bytes);
  if(header.type==='info')socket.end();
 }
});
await new Promise((resolve,reject)=>{socket.once('connect',resolve);socket.once('error',reject)});
socket.setNoDelay(true);
socket.write(encode('synthesize-start',{voice:{name:'F1',language:'de'}}));
socket.write(encode('synthesize-chunk',{text:'Guten Morgen. D'}));
const variant=process.env.FRAGMENT_CASE||'header';
const body=Buffer.from(JSON.stringify({text:text.slice('Guten Morgen. D'.length)}));
const payload=Buffer.from(Array.from({length:257},(_,i)=>i%256));
const head=Buffer.from(JSON.stringify({type:'synthesize-chunk',data_length:body.length,payload_length:payload.length})+'\n');
const tail=Buffer.concat([head,body,payload]);
const cut=variant==='boundary'?head.length:variant==='json'?head.length+17:variant==='payload'?head.length+body.length+37:10;
socket.write(tail.subarray(0,cut));
// First phrase finishes while the following JSON header remains incomplete.
await new Promise(r=>setTimeout(r,2500));
if(variant==='tiny'){for(const byte of tail.subarray(cut)){socket.write(Buffer.from([byte]));await new Promise(r=>setTimeout(r,1));}}else socket.write(tail.subarray(cut));
socket.write(encode('synthesize',{text,voice:{name:'F1',language:'de'}}));
socket.write(encode('synthesize-stop'));
// Verify the stream reader leaves subsequent requests on this connection intact.
socket.write(encode('describe'));
await end;
const pcm=Buffer.concat(payloads);
for(const required of ['audio-start','audio-stop','synthesize-stopped','info']){
 if(events.filter(e=>e===required).length!==1)failure||='missing/duplicated '+required;
}
const expectedBytes=process.argv[4]==='full'?522130:560534;
if(pcm.length!==expectedBytes)failure||='incomplete/duplicated PCM: '+pcm.length;
const result={status:failure?'failed':'passed',failure:failure||null,fragment_case:variant,fragment_delay_seconds:2.5,events,pcm_bytes:pcm.length};
fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n');
if(!failure&&wavPath){
 const h=Buffer.alloc(44);h.write('RIFF');h.writeUInt32LE(36+pcm.length,4);h.write('WAVEfmt ',8);h.writeUInt32LE(16,16);h.writeUInt16LE(1,20);h.writeUInt16LE(1,22);h.writeUInt32LE(44100,24);h.writeUInt32LE(88200,28);h.writeUInt16LE(2,32);h.writeUInt16LE(16,34);h.write('data',36);h.writeUInt32LE(pcm.length,40);fs.writeFileSync(wavPath,Buffer.concat([h,pcm]));
}
if(failure)throw Error(failure);
console.log('Fragmented TCP and post-stream Describe passed');
