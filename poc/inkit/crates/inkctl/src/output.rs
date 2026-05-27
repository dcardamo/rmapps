use serde::Serialize;

#[derive(Serialize)]
pub struct OkEnv<T: Serialize> {
    pub ok: bool,
    pub data: T,
}

#[derive(Serialize)]
pub struct ErrEnv {
    pub ok: bool,
    pub error: ErrorBody,
}

#[derive(Serialize)]
pub struct ErrorBody {
    pub kind: String,
    pub message: String,
}

#[allow(dead_code)]
pub fn print_ok<T: Serialize>(data: T) -> ! {
    let env = OkEnv { ok: true, data };
    println!("{}", serde_json::to_string(&env).unwrap());
    std::process::exit(0);
}

pub fn print_err(kind: &str, message: impl ToString) -> ! {
    let env = ErrEnv {
        ok: false,
        error: ErrorBody {
            kind: kind.to_string(),
            message: message.to_string(),
        },
    };
    println!("{}", serde_json::to_string(&env).unwrap());
    std::process::exit(1);
}
