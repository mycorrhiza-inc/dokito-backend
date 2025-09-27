use aide::axum::{
    ApiRouter,
    routing::{delete, post, post_with},
};

use crate::server::direct_file_fetch::{
    handle_directly_process_file_request, handle_directly_process_file_request_docs,
};
use crate::server::queue_routes;
use crate::server::reprocess_all_handlers::reprocess_dockets;
use crate::server::s3_routes;
use crate::server::temporary_routes::define_temporary_routes;

pub fn create_admin_router() -> ApiRouter {
    let admin_routes = ApiRouter::new()
        .api_route(
            "/cases/{state}/{jurisdiction_name}/manual_process_raw_dockets",
            post(queue_routes::manual_fully_process_dockets_right_now),
        )
        .api_route("/cases/reprocess_dockets_for_all", post(reprocess_dockets))
        .api_route(
            "/direct_file_attachment_process",
            post_with(
                handle_directly_process_file_request,
                handle_directly_process_file_request_docs,
            ),
        )
        .api_route(
            "/cases/{state}/{jurisdiction_name}/purge_all",
            delete(s3_routes::recursive_delete_all_jurisdiction_data),
        )
        .api_route(
            "/docket-process/{state}/{jurisdiction_name}/raw-dockets",
            post(queue_routes::raw_dockets_endpoint),
        )
        .api_route(
            "/docket-process/{state}/{jurisdiction_name}/by-ids",
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
