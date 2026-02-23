use crate::*;
use cpal::traits::HostTrait;
impl Codex {
    pub fn playback(&self) -> R<()> {
        let host = cpal::default_host();
        let _device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("no output device available"))?;
        Ok(())
    }
}
