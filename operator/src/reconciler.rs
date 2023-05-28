use crate::sftpgo_multi_client::SftpgoMultiClient;
use crate::viper_environment_serializer::ViperEnvironmentSerializerError;
use futures::stream::StreamExt;
use futures::TryFuture;
use kube::runtime::controller::Action;
use kube::runtime::watcher::Config;
use kube::runtime::Controller;
use kube::{Api, Client, CustomResourceExt, Resource};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

pub async fn make_reconciler<TResource, ReconcilerFut, ReconcilerFn, CustomizeFn>(
    kubernetes_client: Client,
    recon: ReconcilerFn,
    customize_controller: CustomizeFn,
) where
    TResource:
        Clone + Resource + CustomResourceExt + DeserializeOwned + Debug + Send + Sync + 'static,
    TResource::DynamicType: Debug + Unpin + Eq + Hash + Clone + Default,
    ReconcilerFn: FnMut(Arc<TResource>, Arc<ContextData>) -> ReconcilerFut,
    ReconcilerFut: TryFuture<Ok = Action, Error = Error> + Send + 'static,
    CustomizeFn: FnOnce(Controller<TResource>) -> Controller<TResource>,
{
    info!("Starting reconciler for {:#?}", TResource::crd_name());

    let crd_api = Api::all(kubernetes_client.clone());
    let context = Arc::new(ContextData {
        kubernetes_client: kubernetes_client.clone(),
        sftpgo_client: SftpgoMultiClient::new(),
    });

    let mut controller_setup: Controller<TResource> =
        Controller::new(crd_api.clone(), Config::default());

    controller_setup = customize_controller(controller_setup);

    controller_setup
        .run(recon, error_policy, context)
        .for_each(|res| async move {
            match res {
                Ok(o) => debug!("reconciled: {:?}", o),
                Err(e) => error!("reconcile failed: {}", e),
            }
        })
        .await
}

fn error_policy<TResource: Debug>(
    echo: Arc<TResource>,
    error: &Error,
    _context: Arc<ContextData>,
) -> Action {
    error!("Reconciliation error:\n{:?}.\n{:?}", error, echo);
    Action::requeue(Duration::from_secs(15))
}

pub struct ContextData {
    pub kubernetes_client: Client,
    pub sftpgo_client: SftpgoMultiClient,
}

/// All errors possible to occur during reconciliation
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Any error originating from the `kube-rs` crate
    #[error("Kubernetes reported error: {source}")]
    Kube {
        #[from]
        source: kube::Error,
    },

    #[error("Viper serializer reported error: {source}")]
    ViperSerializer {
        #[from]
        source: ViperEnvironmentSerializerError,
    },

    /// Error in user input or resource definition, typically missing fields.
    #[error("Invalid CRD: {0}")]
    UserInput(String),

    #[error("Sftpgo client reported error: {0}")]
    SftpgoRequestFailed(#[from] sftpgo_client::SftpgoError),

    #[error("Error while decoding base64: {0}")]
    DecodeError(#[from] base64::DecodeError),
}
