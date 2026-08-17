use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CanonicalError {
    #[error("canonical JSON serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// RFC 8785 JSON bytes, suitable for hashing and cross-runtime replay.
pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, CanonicalError> {
    Ok(serde_jcs::to_vec(value)?)
}

pub fn sha256<T: Serialize>(value: &T) -> Result<String, CanonicalError> {
    let bytes = canonical_json(value)?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hash_is_independent_of_object_insertion_order() {
        let left = json!({"b": 2, "a": 1});
        let right = json!({"a": 1, "b": 2});
        assert_eq!(sha256(&left).unwrap(), sha256(&right).unwrap());
    }
}
