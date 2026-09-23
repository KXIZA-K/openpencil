/// Serialises the tests that mutate `jian_ops_schema::image_thumbs`.
///
/// The registry is process-global and both `restore_snapshot` and
/// `clear_registry` replace the whole table, so two such tests running in
/// parallel wipe each other's entries — a failure with nothing to do with what
/// either test asserts. Unique ids are not enough; the replacement is
/// wholesale.
#[cfg(test)]
pub(crate) fn lock_image_thumb_registry() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

