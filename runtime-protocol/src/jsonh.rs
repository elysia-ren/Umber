//! JSON 取值小工具：减少适配器代码的样板。

use serde_json::Value;

pub fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key)
}

pub fn as_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}

pub fn as_u64(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|x| x.as_u64())
}

pub fn as_array<'a>(v: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    v.get(key).and_then(|x| x.as_array())
}

pub fn at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = v;
    for key in path {
        cur = cur.get(key)?;
    }
    Some(cur)
}

pub fn at_str<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    at(v, path).and_then(|x| x.as_str())
}

pub fn at_u64(v: &Value, path: &[&str]) -> Option<u64> {
    at(v, path).and_then(|x| x.as_u64())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn path_access() {
        let v = json!({"a": {"b": [{"c": 7}]}});
        assert_eq!(at_u64(&v, &["a", "b", "0", "c"]), None); // 数组下标不走 get
        assert_eq!(at_str(&json!({"x":{"y":"z"}}), &["x", "y"]), Some("z"));
        assert_eq!(as_u64(&json!({"n": 3}), "n"), Some(3));
    }
}
