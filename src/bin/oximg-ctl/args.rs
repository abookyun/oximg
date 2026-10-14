//! argv → global `Opts` plus one `Cmd`. Every malformed flag is a usage
//! error (exit 2) before anything spawns.

use crate::error::CtlError;
use oximg::pipeline::ImageFormat;
use std::path::PathBuf;

/// Global flags, shared by every command.
pub struct Opts {
    pub bin: Option<PathBuf>,
    pub images_dir: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub pretty: bool,
    pub dry_run: bool,
}

pub enum Cmd {
    Serve(ServeArgs),
    Get(GetArgs),
    Probe(ProbeArgs),
    Resize(ResizeArgs),
    Sign(SignArgs),
    Matrix(MatrixArgs),
}

pub struct ServeArgs {
    pub port: Option<u16>,
}

pub struct GetArgs {
    pub path: String,
    pub base: Option<String>,
    pub write: Option<PathBuf>,
    pub accept: Option<String>,
    pub expect: Option<u16>,
    pub timeout_secs: u64,
}

pub struct ProbeArgs {
    pub file: PathBuf,
}

pub struct ResizeArgs {
    pub input: PathBuf,
    pub w: String,
    pub h: String,
    pub out: Option<PathBuf>,
    pub format: Option<String>,
    pub quality: Option<String>,
    pub preset: Option<String>,
}

pub struct SignArgs {
    pub path: String,
    pub key: Option<String>,
    pub salt: Option<String>,
}

pub struct MatrixArgs {
    pub sources: Vec<String>,
    pub boxes: Vec<(u32, u32)>,
    pub formats: Vec<String>,
    pub negatives: bool,
    pub base: Option<String>,
    pub timeout_secs: u64,
}

pub fn parse_args(args: &[String]) -> Result<(Opts, Cmd), CtlError> {
    let mut bin = None;
    let mut images_dir = None;
    let mut env = Vec::new();
    let mut pretty = false;
    let mut dry_run = false;
    let mut rest: &[String] = args;

    while let Some(a) = rest.first() {
        match a.as_str() {
            "--bin" => {
                bin = Some(PathBuf::from(need_val("--bin", rest)?));
                rest = &rest[2..];
            }
            "--images-dir" => {
                images_dir = Some(PathBuf::from(need_val("--images-dir", rest)?));
                rest = &rest[2..];
            }
            "--env" => {
                let v = need_val("--env", rest)?;
                let (k, val) = v
                    .split_once('=')
                    .ok_or_else(|| CtlError::usage(format!("--env needs KEY=VAL, got {v:?}")))?;
                if k.eq_ignore_ascii_case("PORT") {
                    return Err(CtlError::usage(
                        "--env PORT is reserved; use --port (auto-spawn uses PORT=0)",
                    ));
                }
                if k.eq_ignore_ascii_case("IMAGES_DIR") {
                    return Err(CtlError::usage(
                        "--env IMAGES_DIR is reserved; use --images-dir",
                    ));
                }
                env.push((canonical_env_key(k), val.to_string()));
                rest = &rest[2..];
            }
            "--pretty" => {
                pretty = true;
                rest = &rest[1..];
            }
            "--dry-run" => {
                dry_run = true;
                rest = &rest[1..];
            }
            _ => break,
        }
    }

    let Some((cmd, tail)) = rest.split_first() else {
        return Err(CtlError::usage("missing command"));
    };
    let cmd = match cmd.as_str() {
        "serve" => Cmd::Serve(parse_serve(tail)?),
        "get" => Cmd::Get(parse_get(tail)?),
        "probe" => Cmd::Probe(parse_probe(tail)?),
        "resize" => Cmd::Resize(parse_resize(tail)?),
        "sign" => Cmd::Sign(parse_sign(tail)?),
        "matrix" => Cmd::Matrix(parse_matrix(tail)?),
        other => {
            return Err(CtlError::usage(format!(
                "unknown command {other:?} (serve|get|probe|resize|sign|matrix)"
            )));
        }
    };
    let opts = Opts {
        bin,
        images_dir,
        env,
        pretty,
        dry_run,
    };
    Ok((opts, cmd))
}

fn need_val<'a>(flag: &str, rest: &'a [String]) -> Result<&'a str, CtlError> {
    rest.get(1)
        .map(String::as_str)
        .ok_or_else(|| CtlError::usage(format!("{flag} needs a value")))
}

/// Unix env keys are case-sensitive. Fold the knobs we ourselves set
/// so `--env oximg_bind=::1` actually reaches the child.
fn canonical_env_key(k: &str) -> String {
    for name in [
        "OXIMG_BIND",
        "OXIMG_WORKERS",
        "OXIMG_KEY",
        "OXIMG_SALT",
        "OXIMG_SOURCE_BASE_URL",
        "OXIMG_AUTO_ROTATE",
    ] {
        if k.eq_ignore_ascii_case(name) {
            return name.to_string();
        }
    }
    k.to_string()
}

fn parse_serve(args: &[String]) -> Result<ServeArgs, CtlError> {
    let mut port = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" => {
                let v = it
                    .next()
                    .ok_or_else(|| CtlError::usage("--port needs a value"))?;
                port = Some(
                    v.parse()
                        .map_err(|_| CtlError::usage(format!("invalid --port {v:?}")))?,
                );
            }
            other => {
                return Err(CtlError::usage(format!("serve: unknown option {other:?}")));
            }
        }
    }
    Ok(ServeArgs { port })
}

fn parse_get(args: &[String]) -> Result<GetArgs, CtlError> {
    let mut path = None;
    let mut base = None;
    let mut write = None;
    let mut accept = None;
    let mut expect = None;
    let mut timeout_secs = 60u64;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--base" => {
                base = Some(
                    it.next()
                        .ok_or_else(|| CtlError::usage("--base needs a value"))?
                        .clone(),
                );
            }
            "--write" => {
                write = Some(PathBuf::from(
                    it.next()
                        .ok_or_else(|| CtlError::usage("--write needs a value"))?,
                ));
            }
            "--accept" => {
                accept = Some(
                    it.next()
                        .ok_or_else(|| CtlError::usage("--accept needs a value"))?
                        .clone(),
                );
            }
            "--expect" => {
                let v = it
                    .next()
                    .ok_or_else(|| CtlError::usage("--expect needs a value"))?;
                expect = Some(
                    v.parse()
                        .map_err(|_| CtlError::usage(format!("invalid --expect {v:?}")))?,
                );
            }
            "--timeout-secs" => {
                let v = it
                    .next()
                    .ok_or_else(|| CtlError::usage("--timeout-secs needs a value"))?;
                timeout_secs = v
                    .parse()
                    .map_err(|_| CtlError::usage(format!("invalid --timeout-secs {v:?}")))?;
            }
            flag if flag.starts_with('-') && flag.len() > 1 => {
                return Err(CtlError::usage(format!("get: unknown option {flag:?}")));
            }
            p => {
                if path.is_some() {
                    return Err(CtlError::usage("get takes one path"));
                }
                path = Some(p.to_string());
            }
        }
    }
    let path = path.ok_or_else(|| CtlError::usage("usage: oximg-ctl get <path>"))?;
    Ok(GetArgs {
        path,
        base,
        write,
        accept,
        expect,
        timeout_secs,
    })
}

fn parse_probe(args: &[String]) -> Result<ProbeArgs, CtlError> {
    let mut file = None;
    for a in args {
        if a.starts_with('-') && a.len() > 1 {
            return Err(CtlError::usage(format!("probe: unknown option {a:?}")));
        }
        if file.is_some() {
            return Err(CtlError::usage("probe takes one file"));
        }
        file = Some(PathBuf::from(a));
    }
    let file = file.ok_or_else(|| CtlError::usage("usage: oximg-ctl probe <file>"))?;
    Ok(ProbeArgs { file })
}

fn parse_resize(args: &[String]) -> Result<ResizeArgs, CtlError> {
    let mut positional = Vec::new();
    let mut out = None;
    let mut format = None;
    let mut quality = None;
    let mut preset = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => {
                out = Some(PathBuf::from(
                    it.next()
                        .ok_or_else(|| CtlError::usage("--out needs a value"))?,
                ));
            }
            "-f" | "--format" => {
                format = Some(
                    it.next()
                        .ok_or_else(|| CtlError::usage("--format needs a value"))?
                        .clone(),
                );
            }
            "-q" | "--quality" => {
                quality = Some(
                    it.next()
                        .ok_or_else(|| CtlError::usage("--quality needs a value"))?
                        .clone(),
                );
            }
            "--preset" => {
                preset = Some(
                    it.next()
                        .ok_or_else(|| CtlError::usage("--preset needs a value"))?
                        .clone(),
                );
            }
            flag if flag.starts_with('-') && flag.len() > 1 => {
                return Err(CtlError::usage(format!("resize: unknown option {flag:?}")));
            }
            p => positional.push(p.to_string()),
        }
    }
    let [input, w, h] = positional.as_slice() else {
        return Err(CtlError::usage(
            "usage: oximg-ctl resize <in> <max_w> <max_h> [--out PATH]",
        ));
    };
    Ok(ResizeArgs {
        input: PathBuf::from(input),
        w: w.clone(),
        h: h.clone(),
        out,
        format,
        quality,
        preset,
    })
}

fn parse_sign(args: &[String]) -> Result<SignArgs, CtlError> {
    let mut path = None;
    let mut key = None;
    let mut salt = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--key" => {
                key = Some(
                    it.next()
                        .ok_or_else(|| CtlError::usage("--key needs a value"))?
                        .clone(),
                );
            }
            "--salt" => {
                salt = Some(
                    it.next()
                        .ok_or_else(|| CtlError::usage("--salt needs a value"))?
                        .clone(),
                );
            }
            flag if flag.starts_with('-') && flag.len() > 1 => {
                return Err(CtlError::usage(format!("sign: unknown option {flag:?}")));
            }
            p => {
                if path.is_some() {
                    return Err(CtlError::usage("sign takes one path"));
                }
                path = Some(p.to_string());
            }
        }
    }
    let path = path.ok_or_else(|| CtlError::usage("usage: oximg-ctl sign <path>"))?;
    Ok(SignArgs { path, key, salt })
}

fn parse_matrix(args: &[String]) -> Result<MatrixArgs, CtlError> {
    let mut sources = Vec::new();
    let mut boxes = Vec::new();
    let mut formats = Vec::new();
    let mut negatives = true;
    let mut base = None;
    let mut timeout_secs = 60u64;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--source" => {
                sources.push(
                    it.next()
                        .ok_or_else(|| CtlError::usage("--source needs a value"))?
                        .clone(),
                );
            }
            "--box" => {
                let v = it
                    .next()
                    .ok_or_else(|| CtlError::usage("--box needs WxH"))?;
                boxes.push(parse_box(v)?);
            }
            "--format" => {
                let v = it
                    .next()
                    .ok_or_else(|| CtlError::usage("--format needs a value"))?;
                match v.as_str() {
                    "" | "source" => formats.push(v.clone()),
                    tok if ImageFormat::from_token(tok).is_some() => formats.push(v.clone()),
                    other => {
                        return Err(CtlError::usage(format!(
                            "unknown --format {other:?} (source|{})",
                            ImageFormat::output_token_hint()
                        )));
                    }
                }
            }
            "--no-negatives" => negatives = false,
            "--base" => {
                base = Some(
                    it.next()
                        .ok_or_else(|| CtlError::usage("--base needs a value"))?
                        .clone(),
                );
            }
            "--timeout-secs" => {
                let v = it
                    .next()
                    .ok_or_else(|| CtlError::usage("--timeout-secs needs a value"))?;
                timeout_secs = v
                    .parse()
                    .map_err(|_| CtlError::usage(format!("invalid --timeout-secs {v:?}")))?;
            }
            other => {
                return Err(CtlError::usage(format!("matrix: unknown option {other:?}")));
            }
        }
    }
    Ok(MatrixArgs {
        sources,
        boxes,
        formats,
        negatives,
        base,
        timeout_secs,
    })
}

fn parse_box(v: &str) -> Result<(u32, u32), CtlError> {
    let (w, h) = v
        .split_once('x')
        .ok_or_else(|| CtlError::usage(format!("--box needs WxH, got {v:?}")))?;
    let w: u32 = w
        .parse()
        .map_err(|_| CtlError::usage(format!("invalid --box {v:?}")))?;
    let h: u32 = h
        .parse()
        .map_err(|_| CtlError::usage(format!("invalid --box {v:?}")))?;
    if w == 0 && h == 0 {
        return Err(CtlError::usage(
            "--box 0x0 is invalid (HTTP 400); leave one axis 0 to unconstrained",
        ));
    }
    if w > 8192 || h > 8192 {
        return Err(CtlError::usage(
            "--box axes are 1–8192 (0 allowed on one axis only)",
        ));
    }
    Ok((w, h))
}
