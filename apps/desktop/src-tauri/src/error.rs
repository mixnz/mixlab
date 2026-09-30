//! What a failed command sends back: a key into the translations plus the values to fill it with,
//! rather than an English sentence.
//!
//! The backend is where things go wrong, but it is not where the user is told about it. Writing
//! the message here would mean writing it in one language, and MixLab is bilingual — so what
//! crosses the boundary is `{ code, params }`, and `src/errors.ts` turns that into the sentence
//! the user reads in whichever language the app is set to.
//!
//! A driver's own message (`ERROR 1064 ... You have an error in your SQL syntax`) is not
//! translated: it is the server talking, it is what a search engine and the server's manual both
//! index, and rewording it would only make it harder to act on. Such a message travels as the
//! `message` parameter of whichever code carries it.

use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Display;

/// One failure, in the form the frontend can translate.
///
/// `code` is a dotted key under `error.*` in `src/i18n/en.ts` — which is the source of truth for
/// the key set, so a code with no entry there shows up as its own key in the UI rather than as a
/// crash.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: &'static str,
    /// The values the message interpolates, by the name the translation spells them with.
    pub params: BTreeMap<&'static str, String>,
    /// The failure this one is about, when it wraps another: "Row 3: …" is one code saying which
    /// row and another saying what the server made of it. The frontend translates the inner one
    /// and hands it to the outer as `{{cause}}`, so both halves are in the user's language.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Box<AppError>>,
}

impl AppError {
    pub fn new(code: &'static str) -> Self {
        Self {
            code,
            params: BTreeMap::new(),
            cause: None,
        }
    }

    /// Adds one interpolation value, e.g. `.with("table", name)` for a message reading
    /// `"{{table}} has no primary key"`.
    pub fn with(mut self, key: &'static str, value: impl Display) -> Self {
        self.params.insert(key, value.to_string());
        self
    }

    /// Puts another failure inside this one — see {@link AppError::cause}.
    pub fn caused_by(mut self, cause: AppError) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }
}

impl Display for AppError {
    /// For logs and for `?` into another error — the frontend never sees this form.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)?;
        for (key, value) in &self.params {
            write!(f, " {key}={value}")?;
        }
        if let Some(cause) = &self.cause {
            write!(f, " <- {cause}")?;
        }
        Ok(())
    }
}

/// The shorthand every call site uses: `err!("error.unknownConnection")`, or with parameters
/// `err!("error.cannotWriteFile", path = path.display(), message = e)`.
#[macro_export]
macro_rules! err {
    ($code:literal) => {
        $crate::error::AppError::new($code)
    };
    ($code:literal, $($key:ident = $value:expr),+ $(,)?) => {
        $crate::error::AppError::new($code)
            $(.with(stringify!($key), $value))+
    };
}

#[cfg(test)]
mod tests {
    use crate::error::AppError;

    #[test]
    fn params_are_carried_by_name() {
        let error = err!("error.cannotWriteFile", path = "/tmp/x", message = "denied");
        assert_eq!(error.code, "error.cannotWriteFile");
        assert_eq!(error.params.get("path"), Some(&"/tmp/x".to_string()));
        assert_eq!(error.params.get("message"), Some(&"denied".to_string()));
    }

    #[test]
    fn a_code_without_params_carries_none() {
        assert_eq!(
            err!("error.unknownConnection"),
            AppError::new("error.unknownConnection")
        );
        assert!(err!("error.unknownConnection").params.is_empty());
    }

    /// What the frontend receives: the code and its parameters, camelCased like every other
    /// payload crossing the boundary.
    #[test]
    fn serialises_as_code_and_params() {
        let json = serde_json::to_value(err!("error.rowsMatched", matched = 3)).unwrap();
        assert_eq!(json["code"], "error.rowsMatched");
        assert_eq!(json["params"]["matched"], "3");
    }
}
