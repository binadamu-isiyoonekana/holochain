use super::HostContext;
use super::WasmRibosome;
use holochain_zome_types::SignInput;
use holochain_zome_types::SignOutput;
use std::sync::Arc;

pub async fn sign(
    _ribosome: Arc<WasmRibosome>,
    _host_context: Arc<HostContext>,
    _input: SignInput,
) -> SignOutput {
    unimplemented!();
}