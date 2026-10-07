use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SynthesisOptions {
    pub speed: f32,
    pub steps: usize,
}
impl SynthesisOptions {
    pub fn validate(self) -> Result<Self> {
        ensure!(
            self.speed.is_finite() && (0.8..=1.2).contains(&self.speed),
            "speed must be finite and between 0.8 and 1.2 (inclusive), got {}",
            self.speed
        );
        supertonic3_tts::validate_voice_quality(self.steps).context("invalid steps")?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceProfile {
    pub voice: String,
    pub speed: Option<f32>,
    pub steps: Option<usize>,
}
impl VoiceProfile {
    fn effective(&self, defaults: SynthesisOptions) -> Result<SynthesisOptions> {
        SynthesisOptions {
            speed: self.speed.unwrap_or(defaults.speed),
            steps: self.steps.unwrap_or(defaults.steps),
        }
        .validate()
    }
}
pub type VoiceProfiles = BTreeMap<String, VoiceProfile>;

pub fn parse_profiles(raw: &str, defaults: SynthesisOptions) -> Result<VoiceProfiles> {
    let profiles: VoiceProfiles =
        serde_json::from_str(raw).context("invalid TTS_VOICE_PROFILES JSON")?;
    for (name, profile) in &profiles {
        ensure!(
            !name.trim().is_empty() && name == name.trim() && name.len() <= 128,
            "invalid TTS_VOICE_PROFILES name: {name:?}"
        );
        profile
            .effective(defaults)
            .with_context(|| format!("invalid TTS_VOICE_PROFILES profile {name:?}"))?;
    }
    Ok(profiles)
}

// Validate against real voices before inserting anything: no shadowing or profile chains.
pub fn install_profiles<T: Clone>(
    voices: &mut BTreeMap<String, T>,
    profiles: &VoiceProfiles,
) -> Result<()> {
    for (name, profile) in profiles {
        ensure!(
            !voices.contains_key(name),
            "voice profile {name:?} shadows an existing voice"
        );
        ensure!(
            voices.contains_key(&profile.voice),
            "voice profile {name:?} refers to unknown base voice {:?}",
            profile.voice
        );
    }
    for (name, profile) in profiles {
        voices.insert(name.clone(), voices[&profile.voice].clone());
    }
    Ok(())
}

pub fn resolve<'a>(
    data: &'a Value,
    default_voice: &'a str,
    defaults: SynthesisOptions,
    profiles: &VoiceProfiles,
) -> Result<(&'a str, SynthesisOptions)> {
    for fields in [data, &data["voice"], &data["options"]] {
        ensure!(
            fields.get("speed").is_none() && fields.get("steps").is_none(),
            "Wyoming has no speed/steps fields; select a configured TTS_VOICE_PROFILES voice instead"
        );
    }
    let voice = match data.get("voice") {
        None | Some(Value::Null) => default_voice,
        Some(value) => {
            ensure!(value.is_object(), "voice must be an object");
            match value.get("name") {
                None | Some(Value::Null) => default_voice,
                Some(name) => name.as_str().context("voice.name must be a string")?,
            }
        }
    };
    let options = match profiles.get(voice) {
        Some(profile) => profile.effective(defaults)?,
        None => defaults.validate()?,
    };
    Ok((voice, options))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn defaults() -> SynthesisOptions {
        SynthesisOptions {
            speed: 1.0,
            steps: 6,
        }
    }
    #[test]
    fn validates_speed_and_runtime_step_boundaries() {
        for speed in [0.8, 0.9, 1.0, 1.1, 1.2] {
            assert!(
                SynthesisOptions {
                    speed,
                    ..defaults()
                }
                .validate()
                .is_ok()
            );
        }
        for speed in [0.79, 1.21, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(
                SynthesisOptions {
                    speed,
                    ..defaults()
                }
                .validate()
                .is_err()
            );
        }
        for steps in [5, 6, supertonic3_tts::MAX_VOICE_QUALITY] {
            assert!(
                SynthesisOptions {
                    steps,
                    ..defaults()
                }
                .validate()
                .is_ok()
            );
        }
        for steps in [
            0,
            supertonic3_tts::MIN_VOICE_QUALITY - 1,
            supertonic3_tts::MAX_VOICE_QUALITY + 1,
        ] {
            assert!(
                SynthesisOptions {
                    steps,
                    ..defaults()
                }
                .validate()
                .is_err()
            );
        }
    }
    #[test]
    fn profiles_override_only_specified_values_without_leaking() {
        let profiles = parse_profiles(
            r#"{"slow":{"voice":"F1","speed":0.9},"quality":{"voice":"F1","steps":5}}"#,
            defaults(),
        )
        .unwrap();
        let slow = json!({"voice":{"name":"slow"}});
        assert_eq!(
            resolve(&slow, "F1", defaults(), &profiles).unwrap().1,
            SynthesisOptions {
                speed: 0.9,
                steps: 6
            }
        );
        let quality = json!({"voice":{"name":"quality"}});
        assert_eq!(
            resolve(&quality, "F1", defaults(), &profiles).unwrap().1,
            SynthesisOptions {
                speed: 1.0,
                steps: 5
            }
        );
        assert_eq!(
            resolve(&json!({}), "F1", defaults(), &profiles).unwrap().1,
            defaults()
        );
        assert_eq!(
            resolve(&json!({}), "slow", defaults(), &profiles)
                .unwrap()
                .1
                .speed,
            0.9
        );
    }
    #[test]
    fn rejects_invalid_profiles_and_unstandardized_request_options() {
        for raw in [
            r#"{"bad":{"voice":"F1","speed":0.79}}"#,
            r#"{"bad":{"voice":"F1","speed":1.21}}"#,
            r#"{"bad":{"voice":"F1","steps":4}}"#,
            r#"{"bad":{"voice":"F1","steps":13}}"#,
            r#"{"bad":{"voice":"F1","steps":5.5}}"#,
            r#"{"bad":{"voice":"F1","typo":1}}"#,
        ] {
            assert!(parse_profiles(raw, defaults()).is_err());
        }
        for data in [
            json!({"speed":0.9}),
            json!({"steps":5}),
            json!({"voice":{"speed":1.21}}),
            json!({"options":{"steps":5}}),
            json!({"voice":"F1"}),
        ] {
            assert!(resolve(&data, "F1", defaults(), &VoiceProfiles::new()).is_err());
        }
    }
    #[test]
    fn profiles_reuse_real_voice_and_reject_shadowing_missing_bases_and_chains() {
        let mut voices = BTreeMap::from([("F1".to_owned(), 42)]);
        let profiles =
            parse_profiles(r#"{"slow":{"voice":"F1","speed":0.9}}"#, defaults()).unwrap();
        install_profiles(&mut voices, &profiles).unwrap();
        assert_eq!(voices["slow"], voices["F1"]);
        for raw in [
            r#"{"F1":{"voice":"F1"}}"#,
            r#"{"bad":{"voice":"missing"}}"#,
            r#"{"a":{"voice":"F1"},"b":{"voice":"a"}}"#,
        ] {
            let mut real = BTreeMap::from([("F1".to_owned(), 42)]);
            assert!(
                install_profiles(&mut real, &parse_profiles(raw, defaults()).unwrap()).is_err()
            );
            assert_eq!(real.len(), 1);
        }
    }
}
