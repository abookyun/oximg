//! Path A: oximg's pipeline behind a workers-rs fetch handler.
//!
//! POST an image body to `/?w=400&h=400&fmt=webp&q=80`; the response is
//! the resized image. Everything runs in one wasm32-unknown-unknown
//! module that worker-build wraps with wasm-bindgen glue.

use oximg::pipeline::{self, ImageFormat, Params};
use worker::*;

mod wasi_stubs;

fn params(url: &Url) -> std::result::Result<Params, String> {
    let mut p = Params::default();
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "w" => p.max_width = v.parse().map_err(|_| format!("bad w={v}"))?,
            "h" => p.max_height = v.parse().map_err(|_| format!("bad h={v}"))?,
            "q" => p.quality = v.parse().map_err(|_| format!("bad q={v}"))?,
            "fmt" => p.output = Some(ImageFormat::from_token(&v).ok_or(format!("bad fmt={v}"))?),
            _ => {}
        }
    }
    Ok(p)
}

// panic=abort on this target: without the hook a panic is a bare
// "unreachable" trap with no message.
#[event(start)]
fn start() {
    console_error_panic_hook::set_once();
}

#[event(fetch)]
async fn fetch(mut req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    if req.method() != Method::Post {
        return Response::error("POST an image body to /?w=&h=&fmt=&q=", 405);
    }
    let p = match params(&req.url()?) {
        Ok(p) => p,
        Err(e) => return Response::error(e, 400),
    };
    let src = req.bytes().await?;
    let t0 = Date::now().as_millis();
    match pipeline::process(&src, &p) {
        Ok((out, fmt)) => {
            let ms = Date::now().as_millis() - t0;
            let mut resp = Response::from_bytes(out)?;
            let h = resp.headers_mut();
            h.set("content-type", fmt.content_type())?;
            h.set("x-oximg-pipeline-ms", &ms.to_string())?;
            h.set("x-oximg-path", "workers-rs")?;
            Ok(resp)
        }
        Err(e) => Response::error(format!("{:?}: {e}", e.kind()), 422),
    }
}
