// Vigil
//
// Microservices Status Page
// Copyright: 2021, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use actix_files::NamedFile;
use actix_web::{get, web, web::Data, web::Json, HttpResponse};
use tera::Tera;

use super::context::{IndexContext, INDEX_CONFIG, INDEX_ENVIRONMENT};
use super::payload::ReporterPayload;
use crate::prober::manager::{run_dispatch_plugins, STORE as PROBER_STORE};
use crate::prober::report::{
    handle_flush as handle_flush_report, handle_health as handle_health_report,
    handle_load as handle_load_report, HandleFlushError, HandleHealthError, HandleLoadError,
};
use crate::APP_CONF;

#[get("/")]
async fn index(tera: Data<Tera>) -> HttpResponse {
    // Notice acquire lock in a block to release it ASAP (ie. before template renders)
    let context = {
        IndexContext {
            states: &PROBER_STORE.read().unwrap().states,
            environment: &*INDEX_ENVIRONMENT,
            config: &*INDEX_CONFIG,
        }
    };
    let render = tera.render(
        "index.tera",
        &tera::Context::from_serialize(context).unwrap(),
    );
    if let Ok(s) = render {
        HttpResponse::Ok().content_type("text/html").body(s)
    } else {
        HttpResponse::InternalServerError().body(format!("Template Error {:?}", render))
    }
}

#[get("/robots.txt")]
async fn robots() -> Option<NamedFile> {
    NamedFile::open(APP_CONF.assets.path.join("public").join("robots.txt")).ok()
}

#[get("/status/text")]
async fn status_text() -> &'static str {
    &PROBER_STORE.read().unwrap().states.status.as_str()
}

#[get("/badge/{kind}")]
async fn badge(web::Path(kind): web::Path<String>) -> Option<NamedFile> {
    // Notice acquire lock in a block to release it ASAP (ie. before OS access to file)
    let status = { &PROBER_STORE.read().unwrap().states.status.as_str() };

    NamedFile::open(
        APP_CONF
            .assets
            .path
            .join("images")
            .join("badges")
            .join(format!("{}-{}-default.svg", kind, status)),
    )
    .ok()
}

#[get("/assets/fonts/{folder}/{file}")]
async fn assets_fonts(web::Path((folder, file)): web::Path<(String, String)>) -> Option<NamedFile> {
    NamedFile::open(APP_CONF.assets.path.join("fonts").join(folder).join(file)).ok()
}

#[get("/assets/images/{folder}/{file}")]
async fn assets_images(
    web::Path((folder, file)): web::Path<(String, String)>,
) -> Option<NamedFile> {
    NamedFile::open(APP_CONF.assets.path.join("images").join(folder).join(file)).ok()
}

#[get("/assets/stylesheets/{file}")]
async fn assets_stylesheets(web::Path(file): web::Path<String>) -> Option<NamedFile> {
    NamedFile::open(APP_CONF.assets.path.join("stylesheets").join(file)).ok()
}

#[get("/assets/javascripts/{file}")]
async fn assets_javascripts(web::Path(file): web::Path<String>) -> Option<NamedFile> {
    NamedFile::open(APP_CONF.assets.path.join("javascripts").join(file)).ok()
}

pub async fn disable_service(web::Path(service_name): web::Path<String>) -> String {
    let mut found_it = false;
    let store = &mut PROBER_STORE.write().unwrap();
    let states = &store.states;

    for (probe_id, _probe) in states.probes.iter() {
        if probe_id == &service_name {
            found_it = true;
        }
    }

    if found_it == false {
        format!("Could not find service named '{}'", service_name)
    } else {
        let disabled_services = &mut store.disabled_services;
        disabled_services.insert(service_name);
        format!("{:?}", disabled_services)
    }
}

pub async fn enable_service(web::Path(service_name): web::Path<String>) -> String {
	let disabled_services = &mut PROBER_STORE.write().unwrap().disabled_services;
    if disabled_services.contains(&service_name) {
        disabled_services.remove(&service_name);
        format!("{:?}", disabled_services)
    } else {
        format!("ERROR: Could not find disabled service: {:?}", service_name)
    }
}

// Notice: reporter report route is managed in manager due to authentication needs
pub async fn reporter_report(
    web::Path((probe_id, node_id)): web::Path<(String, String)>,
    data: Json<ReporterPayload>,
) -> HttpResponse {
    // Route report to handler (depending on its contents)
    if let Some(ref load) = data.load {
        // Load reports should come for 'push' nodes only
        match handle_load_report(
            &probe_id,
            &node_id,
            &data.replica,
            data.interval,
            load.cpu,
            load.ram,
        ) {
            Ok(forward) => {
                // Trigger a plugins check
                run_dispatch_plugins(&probe_id, &node_id, forward);

                HttpResponse::Ok().finish()
            }
            Err(HandleLoadError::InvalidLoad) => HttpResponse::BadRequest().finish(),
            Err(HandleLoadError::WrongMode) => HttpResponse::PreconditionFailed().finish(),
            Err(HandleLoadError::NotFound) => HttpResponse::NotFound().finish(),
        }
    } else if let Some(ref health) = data.health {
        // Health reports should come for 'local' nodes only
        match handle_health_report(&probe_id, &node_id, &data.replica, data.interval, health) {
            Ok(_) => HttpResponse::Ok().finish(),
            Err(HandleHealthError::WrongMode) => HttpResponse::PreconditionFailed().finish(),
            Err(HandleHealthError::NotFound) => HttpResponse::NotFound().finish(),
        }
    } else {
        // Report contents is invalid
        HttpResponse::BadRequest().finish()
    }
}

// Notice: reporter flush route is managed in manager due to authentication needs
pub async fn reporter_flush(
    web::Path((probe_id, node_id, replica_id)): web::Path<(String, String, String)>,
) -> HttpResponse {
    // Flush reports should come for 'push' and 'local' nodes only
    match handle_flush_report(&probe_id, &node_id, &replica_id) {
        Ok(()) => HttpResponse::Ok().finish(),
        Err(HandleFlushError::WrongMode) => HttpResponse::PreconditionFailed().finish(),
        Err(HandleFlushError::NotFound) => HttpResponse::NotFound().finish(),
    }
}
