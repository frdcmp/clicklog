#![cfg(feature = "actix")]
mod common;

use std::time::Duration;

use actix_web::{test, web, App, HttpMessage, HttpResponse};
use clicklog::actix::{client_errors, AccessLog};
use clicklog::{Config, Who};

struct User(String);

#[actix_web::test]
async fn access_log_and_client_errors() {
    let (url, mut rx) = common::gateway().await;
    let mut config = Config::new(&url, "ik_test");
    config.flush_interval = Duration::from_millis(100);
    assert!(clicklog::init(config));

    let app = test::init_service(
        App::new()
            .wrap(AccessLog::new().who(|req| req.extensions().get::<User>().map(|u| Who::id(&u.0))))
            .route(
                "/join/{token}",
                web::get().to(|req: actix_web::HttpRequest| async move {
                    req.extensions_mut().insert(User("u1".into()));
                    HttpResponse::NotFound().finish()
                }),
            )
            .route("/health", web::get().to(HttpResponse::Ok))
            .route("/telemetry/client", web::post().to(client_errors)),
    )
    .await;

    let get = |path: &str| test::TestRequest::get().uri(path).to_request();
    assert_eq!(test::call_service(&app, get("/join/secret-token")).await.status(), 404);
    assert_eq!(test::call_service(&app, get("/health")).await.status(), 200);
    assert_eq!(test::call_service(&app, get("/nowhere/else")).await.status(), 404);
    let report = test::TestRequest::post()
        .uri("/telemetry/client")
        .insert_header(("cf-connecting-ip", "2.2.2.2"))
        .set_json(serde_json::json!({ "events": [{ "message": "TypeError: x", "route": "/trees" }] }))
        .to_request();
    assert_eq!(test::call_service(&app, report).await.status(), 204);

    let (_, events) = common::events(&mut rx, Duration::from_secs(2)).await;
    let text = serde_json::to_string(&events).unwrap();
    assert!(!text.contains("secret-token"), "no path in the log: {text}");
    let join = events.iter().find(|e| e["route"] == "/join/{token}").expect("the join request");
    assert_eq!(join["http_status"], 404);
    assert_eq!(join["severity"], "warn");
    assert_eq!(join["user_id"], "u1");
    assert!(events.iter().any(|e| e["route"] == "(no route)"));
    assert!(!events.iter().any(|e| e["route"] == "/health"));
    let page = events.iter().find(|e| e["source"] == "frontend").expect("the page's error");
    assert_eq!(page["message"], "TypeError: x");
    assert_eq!(page["ip"], "2.2.2.2");
    assert_eq!(page["category"], "client");
}
