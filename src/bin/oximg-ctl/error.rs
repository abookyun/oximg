//! The one failure shape: a JSON object on stdout and an exit code
//! (1 for a command that ran and failed its proof, 2 for usage).

use serde_json::{Value, json};

pub struct CtlError {
    exit: i32,
    pub json: Value,
    pretty: bool,
}

impl CtlError {
    pub fn usage(msg: impl Into<String>) -> Self {
        let msg = msg.into();
        Self {
            exit: 2,
            json: json!({
                "ok": false,
                "error": msg,
                "hint": "oximg-ctl --help",
            }),
            pretty: false,
        }
    }

    pub fn fail(msg: impl Into<String>, hint: Option<&str>) -> Self {
        let mut v = json!({ "ok": false, "error": msg.into() });
        if let Some(h) = hint {
            v["hint"] = json!(h);
        }
        Self {
            exit: 1,
            json: v,
            pretty: false,
        }
    }

    pub fn from_value(exit: i32, json: Value) -> Self {
        Self {
            exit,
            json,
            pretty: false,
        }
    }

    pub fn with_pretty(mut self, pretty: bool) -> Self {
        self.pretty = pretty;
        self
    }

    pub fn exit(self) -> ! {
        emit(&self.json, self.pretty);
        std::process::exit(self.exit);
    }
}

pub fn emit(v: &Value, pretty: bool) {
    if pretty {
        println!(
            "{}",
            serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
        );
    } else {
        println!("{v}");
    }
}
