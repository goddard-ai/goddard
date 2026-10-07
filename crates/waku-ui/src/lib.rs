// The catalog is embedded once by `waku-localization` and registered into
// `waku-protocol` when linked; `crate::i18n` resolves through `waku-client`.
macro_rules! tr {
    ($key:expr) => {
        waku_localization::translate($key)
    };
    ($key:expr, $($name:ident => $value:expr),+ $(,)?) => {
        waku_localization::translate_args(
            $key,
            &[$( (stringify!($name), $value.to_string()) ),+],
        )
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        waku_localization::translate_args(
            $key,
            &[$( (stringify!($name), $value.to_string()) ),+],
        )
    };
}

pub mod fonts;
pub mod host;
pub mod input;
pub mod md;
pub mod theme;
pub mod ui;
