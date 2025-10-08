//! # Admin Routes - Critical System Endpoints
//!
//! This module contains ALL critical administrative endpoints for the Dokito processing system.
//!
//! ## Key Endpoints Defined Here:
//! - **Direct File Processing**: Immediate file processing without queue
//! - **Docket Processing**: All docket-related batch processing operations
//! - **Temporary Routes**: Development and testing endpoints
//!

use aide::axum::{
    ApiRouter,
    routing::{post, post_with},
};

use crate::server::direct_file_fetch::{
    handle_directly_process_file_request, handle_directly_process_file_request_docs,
};
use crate::server::queue_routes;
use crate::server::temporary_routes::define_temporary_routes;

/// Creates the complete admin router with ALL critical administrative endpoints.
///
/// **IMPORTANT: This function defines ALL admin functionality for the system.**
///
/// ## Endpoints Created:
///
/// ### Direct File Processing
/// - `POST /direct_file_attachment_process` - Process files immediately without queuing
///
/// ### Docket Processing (All Jurisdictions)
/// - `POST /docket-process/{state}/{jurisdiction_name}/raw-dockets` - Process raw docket data
/// - `POST /docket-process/{state}/{jurisdiction_name}/govid/process` - Process docket by government ID
/// - `POST /docket-process/{state}/{jurisdiction_name}/govid/ingest` - Ingest docket by government ID
/// - `POST /docket-process/{state}/{jurisdiction_name}/govid/full` - Full process and ingest by government ID
/// - `POST /docket-process/{state}/{jurisdiction_name}/by-jurisdiction` - Process all dockets by jurisdiction
/// - `POST /docket-process/{state}/{jurisdiction_name}/by-daterange` - Process dockets within date range
///
/// ### Temporary/Development Routes
/// - Various testing and development endpoints (see temporary_routes module)
///
/// ## Returns
/// A fully configured `ApiRouter` with all admin endpoints mounted and documented.
pub fn create_admin_router() -> ApiRouter {
    let admin_routes = ApiRouter::new()
        // Direct file processing - immediate processing without queue
        .api_route(
            "/direct_file_attachment_process",
            post_with(
                handle_directly_process_file_request,
                handle_directly_process_file_request_docs,
            ),
        )
        // Docket processing endpoints - batch operations for all jurisdictions
        .api_route(
            "/docket-process/{state}/{jurisdiction_name}/raw-dockets",
            post(queue_routes::raw_dockets_endpoint),
        )
        // Government ID specific endpoints
        // each takes only a docket_ids: Vec<NonEmptyString>
        .api_route(
            "/docket-process/{state}/{jurisdiction_name}/govid/process",
            post(queue_routes::process_by_govid),
        )
        .api_route(
            "/docket-process/{state}/{jurisdiction_name}/govid/ingest",
            post(queue_routes::ingest_by_govid),
        )
        .api_route(
            "/docket-process/{state}/{jurisdiction_name}/govid/full",
            post(queue_routes::process_and_ingest_by_govid),
        )
        // Bulk processing endpoints - handle multiple dockets at once
        .api_route(
            "/docket-process/{state}/{jurisdiction_name}/by-jurisdiction",
            post(queue_routes::by_jurisdiction_endpoint),
        )
        .api_route(
            "/docket-process/{state}/{jurisdiction_name}/by-daterange",
            post(queue_routes::by_daterange_endpoint),
        );

    // Add temporary/development routes to the admin router
    define_temporary_routes(admin_routes)
}
