#![allow(dead_code)]

use std::{
    convert::{Infallible, identity},
    net::{Ipv4Addr, SocketAddr},
};

use aide::axum::ApiRouter;
use mycorrhiza_common::{
    api_documentation::generate_api_docs_and_serve,
    llm_deepinfra::DEEPINFRA_API_KEY,
    otel_tracing::initialize_tracing_and_wrap_router,
    tasks::{routing::define_generic_task_routes, workers::spawn_worker_loop},
};
use tasks::add_user_task_routes;

use crate::types::s3_stuff::DIGITALOCEAN_S3;

mod tasks;
mod types;

