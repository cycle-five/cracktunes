// ------------------------------------------------------------------
// Public types we use to simplify return and parameter types.
// ------------------------------------------------------------------
use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::Arc;
use std::result::Result;
use tokio::sync::{Mutex, RwLock};

pub type Error = Box<dyn StdError + Send + Sync>;
pub type ArcTRwLock<T> = Arc<RwLock<T>>;
pub type ArcTMutex<T> = Arc<Mutex<T>>;
pub type ArcRwMap<K, V> = Arc<RwLock<HashMap<K, V>>>;
pub type ArcTRwMap<K, V> = Arc<RwLock<HashMap<K, V>>>;
pub type ArcMutDMap<K, V> = Arc<Mutex<HashMap<K, V>>>;
// pub type CrackedResult<T> = Result<T, crack_core::CrackedError>;
// pub type CrackedHowResult<T> = anyhow::Result<T, crack_core::CrackedError>;
