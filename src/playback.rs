use crate::*;
use cpal::traits::HostTrait;
impl Codex {
    pub fn playback(&self) {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .expect("no output device available");
    }
}
