//! Per-provider anchor send bodies. Each module owns one provider's request
//! shape + its "did the send consume the window" success check; the dispatcher
//! in `anchor::send` picks the arm and supplies the endpoint URL. Claude has no
//! module here — its arm is a one-call route to `providers::claude::web` kept
//! inline in the dispatcher.

mod codex;
mod zai;

pub use codex::send_codex;
pub use zai::send_zai;
