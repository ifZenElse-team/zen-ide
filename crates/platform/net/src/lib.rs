mod settings;

pub use settings::{ProxyKind, ProxyMode, ProxySettings};

#[cfg(not(target_arch = "wasm32"))]
mod agent;
#[cfg(not(target_arch = "wasm32"))]
mod connector;
#[cfg(not(target_arch = "wasm32"))]
mod keychain;
#[cfg(not(target_arch = "wasm32"))]
mod proxy;
#[cfg(not(target_arch = "wasm32"))]
mod system;
#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-proxy")))]
pub mod test_proxy;
#[cfg(not(target_arch = "wasm32"))]
pub mod trust;

#[cfg(not(target_arch = "wasm32"))]
pub use agent::*;
#[cfg(not(target_arch = "wasm32"))]
pub use proxy::{Route, is_excepted, route};
#[cfg(not(target_arch = "wasm32"))]
pub use system::{SystemProxy, parse_scutil};
