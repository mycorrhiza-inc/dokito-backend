use aide::axum::{
    ApiRouter,
    routing::{post, post_with},
};

use crate::server::direct_file_fetch::{
    handle_directly_process_file_request, handle_directly_process_file_request_docs,
};
use crate::server::queue_routes;
use crate::server::temporary_routes::define_temporary_routes;

pub fn create_admin_router() -> ApiRouter {
    let admin_routes = ApiRouter::new()
        .api_route(
            "/direct_file_attachment_process",
            post_with(
                handle_directly_process_file_request,
                handle_directly_process_file_request_docs,
            ),
        )
        .api_route(
            "/docket-process/{state}/{jurisdiction_name}/raw-dockets",
            post(queue_routes::raw_dockets_endpoint),
        )
        .api_route(
            "/docket-process/{state}/{jurisdiction_name}/by-gov-ids",
            post(queue_routes::by_ids_endpoint),
        )
        .api_route(
            "/docket-process/{state}/{jurisdiction_name}/by-jurisdiction",
            post(queue_routes::by_jurisdiction_endpoint),
        )
        .api_route(
            "/docket-process/{state}/{jurisdiction_name}/by-daterange",
            post(queue_routes::by_daterange_endpoint),
        );

    // Temporary routes are also admin routes
    define_temporary_routes(admin_routes)
}
