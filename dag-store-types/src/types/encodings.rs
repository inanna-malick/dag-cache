use base58;
use base64;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Base58(pub Vec<u8>);

impl Base58 {
    pub fn from_string(x: &str) -> Result<Base58, base58::FromBase58Error> {
        base58::FromBase58::from_base58(x).map(Base58)
    }

    pub fn from_bytes(x: Vec<u8>) -> Base58 {
        Base58(x)
    }
}

impl std::fmt::Debug for Base58 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", base58::ToBase58::to_base58(&self.0[..]))
    }
}

impl std::fmt::Display for Base58 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", base58::ToBase58::to_base58(&self.0[..]))
    }
}

// always serialize as string (json)
impl Serialize for Base58 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Base58 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: &str = &String::deserialize(deserializer)?;
        base58::FromBase58::from_base58(s)
            .map(Base58)
            .map_err(|e| match e {
                base58::FromBase58Error::InvalidBase58Character(c, _) => {
                    de::Error::custom(format!("invalid base58 char {}", c))
                }
                base58::FromBase58Error::InvalidBase58Length => {
                    de::Error::custom("invalid base58 length(?)".to_string())
                }
            })
    }
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Base64(pub Vec<u8>);

// always serialize as string (json)
impl Serialize for Base64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Base64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = String::deserialize(deserializer)?;
        base64::decode(&s).map(Base64).map_err(de::Error::custom)
    }
}
