use std::fmt::Debug;

use futures::channel::mpsc::SendError;
use futures::Stream;
use futures_util::sink::SinkExt;
use log::*;
use thiserror::Error;
use yellowstone_grpc_client::{GeyserGrpcBuilderError, GeyserGrpcClient, GeyserGrpcClientError};
use yellowstone_grpc_proto::prelude::{SubscribeRequest, SubscribeUpdate};
use yellowstone_grpc_proto::tonic::Status;

use crate::{GeyserConfig, TxStub};

pub type GeyserClientResult<T = ()> = Result<T, GeyserClientError>;

#[derive(Debug, Error)]
pub enum GeyserClientError {
  #[error("{0}")]
  GeyserBuilder(#[from] GeyserGrpcBuilderError),

  #[error("{0}")]
  GeyserClient(#[from] GeyserGrpcClientError),

  #[error("{0}")]
  Anyhow(#[from] anyhow::Error),

  #[error("{0}")]
  Send(#[from] SendError),

  #[error("{0}")]
  Channel(#[from] crossbeam::channel::SendError<TxStub>),
}

pub struct GrpcClient {
  pub cfg: GeyserConfig,
}

impl GrpcClient {
  pub fn new(cfg: GeyserConfig) -> Self {
    Self {
      cfg,
    }
  }

  pub async fn subscribe(&self) -> GeyserClientResult<impl Stream<Item=Result<SubscribeUpdate, Status>>> {
    let cfg = self.cfg.clone();
    let x_token: Option<String> = Some(cfg.x_token);
    let mut client = GeyserGrpcClient::build_from_shared(cfg.grpc)?.x_token(x_token)?.connect().await?;
    let (mut subscribe_tx, stream) = client.subscribe().await?;
    subscribe_tx.send(SubscribeRequest::from(self.cfg.clone())).await?;
    Ok(stream)
  }
}