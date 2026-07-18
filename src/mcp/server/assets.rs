// 대시보드 SPA 임베드 자산 서빙: index·favicon·번들 자산·미포함 빌드 안내 페이지.

/// 대시보드 SPA(dashboard 피처) 임베드 자산. release=바이너리 내장, debug=디스크(frontend/dist) 읽기.
/// frontend를 `npm run build`한 뒤 `cargo build --features dashboard`로 임베드한다.
#[cfg(feature = "dashboard")]
#[derive(rust_embed::RustEmbed)]
#[folder = "frontend/dist"]
pub(super) struct DashAssets;

/// 임베드된 SPA 자산 하나를 확장자 기반 MIME으로 서빙한다(없으면 404).
#[cfg(feature = "dashboard")]
pub(super) fn serve_embedded(path: &str) -> axum::response::Response {
    use axum::response::IntoResponse;
    match DashAssets::get(path) {
        Some(content) => (
            [(axum::http::header::CONTENT_TYPE, mime_for_path(path))],
            content.data.into_owned(),
        )
            .into_response(),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

/// 경로 확장자로 정적 자산 Content-Type을 고른다(SPA 번들이 쓰는 종류만, 신규 의존 회피).
#[cfg(feature = "dashboard")]
pub(super) fn mime_for_path(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("json") => "application/json",
        Some("ico") => "image/x-icon",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

/// GET /dashboard: SPA 진입 index.html.
#[cfg(feature = "dashboard")]
pub(super) async fn dashboard_index() -> axum::response::Response {
    serve_embedded("index.html")
}

/// GET /dashboard/favicon.svg: SPA 파비콘.
#[cfg(feature = "dashboard")]
pub(super) async fn dashboard_favicon() -> axum::response::Response {
    serve_embedded("favicon.svg")
}

/// GET /dashboard/assets/{*path}: Vite 번들 자산(js/css/폰트 등).
#[cfg(feature = "dashboard")]
pub(super) async fn dashboard_asset(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> axum::response::Response {
    serve_embedded(&format!("assets/{path}"))
}

/// dashboard 피처 없이 빌드된 경우의 /dashboard 안내 페이지(API events/roster는 그대로 동작).
#[cfg(all(feature = "serve", not(feature = "dashboard")))]
pub(super) async fn dashboard_fallback_page() -> axum::response::Html<&'static str> {
    axum::response::Html(
        "<!DOCTYPE html><html lang=\"ko\"><head><meta charset=\"utf-8\"><title>총감독 대시보드</title></head>\
         <body style=\"font-family:system-ui;margin:2rem\"><h1>대시보드 미포함 빌드</h1>\
         <p>이 바이너리는 <code>dashboard</code> 피처 없이 빌드되었습니다. \
         <code>cargo build --features dashboard</code>로 빌드하거나 release 바이너리를 사용하세요. \
         API <code>/dashboard/events</code>, <code>/dashboard/roster</code>는 동작합니다.</p></body></html>",
    )
}
