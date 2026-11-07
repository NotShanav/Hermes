use crate::usb;
use hermes_crypto::provision::create_key_set;
use hermes_crypto::SecretProtector;
use hermes_db::{Database, OpenOptions};
use std::path::{Path, PathBuf};

pub struct InitInfo {
    pub data_dir: PathBuf,
    pub key_id: String,
    pub share_b: PathBuf,
}

pub struct InitOptions<'a> {
    pub data_dir: &'a Path,
    pub share_b: &'a Path,
    /// Permit the USB key file on the same Windows drive as the data (defeats the two-share design).
    pub allow_same_drive: bool,
}

/// Creates the data directory, the encrypted archive, the key set (Share A protected by `protector`, Share B on
/// the USB path, signing key) and registers the public keys. NEVER overwrites an existing archive's keys.
pub fn init(o: &InitOptions, protector: &dyn SecretProtector) -> Result<InitInfo, String> {
    if usb::same_drive(o.data_dir, o.share_b) && !o.allow_same_drive {
        return Err(format!(
            "the USB key file ({}) is on the same drive as the Hermes data ({}).\nThe whole point of the two shares is that the second one lives on a SEPARATE USB drive. Plug the USB stick in and use --usb E:\\",
            o.share_b.display(),
            o.data_dir.display()
        ));
    }
    std::fs::create_dir_all(o.data_dir).map_err(|e| format!("cannot create {}: {e}", o.data_dir.display()))?;
    let db_path = o.data_dir.join("hermes.db");
    let (mut db, _) = Database::open(&db_path, OpenOptions::default()).map_err(|e| e.to_string())?;
    if !db.keys().map_err(|e| e.to_string())?.is_empty() {
        return Err(format!(
            "Hermes is already initialised in {}: an archive with encryption keys exists.\nIt is never overwritten. To start a new key set for new recordings use `hermes-keytool rotate`; to use a different archive pass --data-dir.",
            o.data_dir.display()
        ));
    }
    if o.share_b.exists() {
        return Err(format!("{} already exists; refusing to overwrite a key file. Choose another path.", o.share_b.display()));
    }
    let nk = create_key_set(&o.data_dir.join("keys"), o.share_b, protector).map_err(|e| e.to_string())?;
    db.register_key(&nk.info, Some(&nk.sign_public), true).map_err(|e| e.to_string())?;
    db.mark_clean_shutdown().map_err(|e| e.to_string())?;
    Ok(InitInfo { data_dir: o.data_dir.to_path_buf(), key_id: nk.info.key_id.to_hex(), share_b: o.share_b.to_path_buf() })
}

pub fn explain(i: &InitInfo) -> String {
    format!(
        "Hermes is initialised.\n\
         \n  Archive (encrypted):      {data}\\hermes.db\
         \n  Key set:                  {key}\
         \n  Share A (this laptop):    {data}\\keys\\  (protected by your Windows login)\
         \n  Share B (USB drive):      {usb}\
         \n\
         \nIMPORTANT: where to keep the USB key file\
         \n  * Keep {usb} on a USB stick that you store AWAY from this laptop when you are not reading the history.\
         \n  * Make a second copy on another stick (it is a small text file). Test it:  hermes check --share-b <copy>\
         \n  * Reading the history needs BOTH shares: Share A (this laptop, this Windows account) AND Share B (the USB file).\
         \n    Recording needs neither: you can start collecting with the USB unplugged.\
         \n  * If you lose the USB file or lose Share A (Windows reinstall, new account) the existing history can NEVER be\
         \n    read again. Back up Share A:  hermes-keytool backup-share-a --data-dir \"{data}\" --key-id {key} --out <file>\
         \n\
         \nNext:  hermes start      (records foreground application names and window titles; Ctrl+C stops)\
         \n       hermes view       (browse, filter and search the history; needs the USB stick)\n",
        data = i.data_dir.display(),
        key = i.key_id,
        usb = i.share_b.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hermes_crypto::protector::TestProtector;

    fn opts<'a>(d: &'a Path, b: &'a Path) -> InitOptions<'a> {
        InitOptions { data_dir: d, share_b: b, allow_same_drive: true }
    }

    #[test]
    fn init_creates_everything_and_explains_where_the_usb_file_goes() {
        let d = tempfile::tempdir().unwrap();
        let usb = tempfile::tempdir().unwrap();
        let b = usb.path().join("hermes-share-b.key");
        let i = init(&opts(d.path(), &b), &TestProtector::new()).unwrap();
        assert!(d.path().join("hermes.db").exists() && b.exists());
        assert!(hermes_crypto::shares::share_a_path(&d.path().join("keys"), &hermes_crypto::KeyId::from_hex(&i.key_id).unwrap()).exists());
        let t = explain(&i);
        assert!(t.contains("AWAY from this laptop") && t.contains("BOTH shares") && t.contains("hermes start") && t.contains(&i.key_id));
        // the key file on the stick holds only Share B (never Share A or a private key)
        let text = std::fs::read_to_string(&b).unwrap();
        assert!(text.starts_with("HERMES-SHARE-B v2") && text.contains("sign_pub:"));
    }

    #[test]
    fn init_never_overwrites_an_existing_archive_or_key_file() {
        let d = tempfile::tempdir().unwrap();
        let usb = tempfile::tempdir().unwrap();
        let prot = TestProtector::new();
        let b = usb.path().join("b.key");
        init(&opts(d.path(), &b), &prot).unwrap();
        let db_before = std::fs::read(d.path().join("hermes.db")).unwrap();
        let share_before = std::fs::read(&b).unwrap();
        // second init, same data dir
        let b2 = usb.path().join("b2.key");
        let e = init(&opts(d.path(), &b2), &prot).err().unwrap();
        assert!(e.contains("already initialised") && e.contains("never overwritten"), "{e}");
        assert!(!b2.exists(), "no new key file may be created");
        assert!(std::fs::read(&b).unwrap() == share_before, "existing key file untouched");
        let d2 = tempfile::tempdir().unwrap();
        // key file path already exists
        let e = init(&opts(d2.path(), &b), &prot).err().unwrap();
        assert!(e.contains("already exists"), "{e}");
        assert!(std::fs::read(&b).unwrap() == share_before);
        let _ = db_before;
    }

    #[test]
    fn init_refuses_to_put_the_usb_share_on_the_same_drive_and_touches_nothing() {
        let prot = TestProtector::new();
        let o = InitOptions { data_dir: Path::new("C:\\HermesCliTestNeverCreated"), share_b: Path::new("c:\\stick.key"), allow_same_drive: false };
        let e = init(&o, &prot).err().unwrap();
        assert!(e.contains("SEPARATE USB drive"), "{e}");
        assert!(!Path::new("C:\\HermesCliTestNeverCreated").exists());
    }
}
