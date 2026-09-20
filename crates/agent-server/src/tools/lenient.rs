//! 宽松参数反序列化：部分 LLM（尤其本地模型）会把工具参数以字符串形式
//! 传给数字/布尔字段（如 `{"secs": "3"}`、`{"dry_run": "true"}`）。
//! serde 默认严格按 JSON 类型校验，数字字段收到字符串会报
//! `-32602 expected u64/boolean`。这里提供一组 `deserialize_with` helper：
//! 字符串能解析成目标类型就接受，解析不了仍报错（不吞掉真实错误）。
//!
//! 用法：`#[serde(default, deserialize_with = "lenient::opt_u64")] secs: Option<u64>`
//! 只对 Option 字段使用（缺省/空串 → None）。

use serde::Deserialize;

fn de_str<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    use serde::de::Error as _;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Str(String),
        Num(serde_json::Value),
        Null,
    }
    match Raw::deserialize(de)? {
        Raw::Null => Ok(None),
        Raw::Str(s) => {
            let s = s.trim();
            if s.is_empty() {
                Ok(None)
            } else {
                s.parse::<T>()
                    .map(Some)
                    .map_err(|e| D::Error::custom(format!("无法解析为数字：{s}（{e}）")))
            }
        }
        Raw::Num(v) => {
            // 数字按字符串再解析，覆盖整数/浮点/科学计数
            let s = v.to_string();
            s.parse::<T>()
                .map(Some)
                .map_err(|e| D::Error::custom(format!("无法解析为数字：{s}（{e}）")))
        }
    }
}

/// `Option<usize>`：接受 `3` / `"3"` / 空串 / null。
pub fn opt_usize<'de, D>(de: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    de_str(de)
}

/// `Option<u64>`：接受 `3` / `"3"` / 空串 / null。
pub fn opt_u64<'de, D>(de: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    de_str(de)
}

/// `Option<u32>`：接受 `1` / `"1"` / 空串 / null。
pub fn opt_u32<'de, D>(de: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    de_str(de)
}

/// `Option<f64>`：接受 `85` / `"85"` / `"85.5"` / 空串 / null。
pub fn opt_f64<'de, D>(de: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    de_str(de)
}

fn de_bool<'de, D>(de: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Bool(bool),
        Str(String),
        Num(serde_json::Value),
        Null,
    }
    match Raw::deserialize(de)? {
        Raw::Null => Ok(None),
        Raw::Bool(b) => Ok(Some(b)),
        Raw::Str(s) => {
            let s = s.trim().to_ascii_lowercase();
            match s.as_str() {
                "" => Ok(None),
                "true" | "1" | "yes" | "on" => Ok(Some(true)),
                "false" | "0" | "no" | "off" => Ok(Some(false)),
                _ => Err(D::Error::custom(format!("无法解析为布尔：{s}"))),
            }
        }
        Raw::Num(v) => {
            let s = v.to_string();
            match s.as_str() {
                "1" => Ok(Some(true)),
                "0" => Ok(Some(false)),
                _ => Err(D::Error::custom(format!("无法解析为布尔：{s}"))),
            }
        }
    }
}

/// `Option<bool>`：接受 `true` / `"true"` / `"1"` / 空串 / null。
pub fn opt_bool<'de, D>(de: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    de_bool(de)
}
