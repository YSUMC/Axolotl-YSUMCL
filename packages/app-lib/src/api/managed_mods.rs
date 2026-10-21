//! Official mod contents come from the hash-verified pack, never from a scan of
//! the user's current mods. Extra files are quarantined, not deleted.
use crate::api::pack::install_from::{EnvType, PackFormat};
use crate::managed::ManagedInstanceSpec;
use crate::state::SideType;
use crate::util::fetch::{fetch, sha1_file_async};
use crate::{ErrorKind, State};
use sha1_smol::Sha1;
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug)]
struct OfficialMod {
    sha1: String,
    size: u64,
    urls: Vec<String>,
    bytes: Option<Vec<u8>>,
}

fn input(message: impl Into<String>) -> crate::Error {
    ErrorKind::InputError(message.into()).into()
}

fn mod_path(value: &str) -> crate::Result<Option<String>> {
    if !value.starts_with("mods/") {
        return Ok(None);
    }
    path_util::SafeRelativeUtf8UnixPathBuf::try_from(value.to_string())
        .map_err(|_| input("官方包包含不安全的模组路径"))?;
    let relative = &value[5..];
    if relative.is_empty()
        || relative.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.contains(['\\', ':'])
                || part.ends_with(['.', ' '])
        })
    {
        return Err(input("官方包包含不安全的模组路径"));
    }
    Ok(Some(relative.to_string()))
}

fn read_official_mods(
    path: &Path,
) -> crate::Result<BTreeMap<String, OfficialMod>> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)
        .map_err(|error| input(error.to_string()))?;
    if archive.len() > 100_000 {
        return Err(input("官方包文件数量超出限制"));
    }
    let mut json = String::new();
    archive
        .by_name("modrinth.index.json")
        .map_err(|error| input(error.to_string()))?
        .take(16 * 1024 * 1024)
        .read_to_string(&mut json)?;
    let pack: PackFormat = serde_json::from_str(&json)?;
    let mut mods = BTreeMap::new();
    for file in pack.files {
        if file.env.as_ref().and_then(|env| env.get(&EnvType::Client))
            == Some(&SideType::Unsupported)
        {
            continue;
        }
        let Some(path) = mod_path(file.path.as_str())? else {
            continue;
        };
        let hash = file
            .hashes
            .get(&crate::api::pack::install_from::PackFileHash::Sha1)
            .filter(|hash| {
                hash.len() == 40 && hash.bytes().all(|c| c.is_ascii_hexdigit())
            })
            .ok_or_else(|| input("官方模组缺少有效 SHA-1"))?
            .to_lowercase();
        if file.file_size > 512 * 1024 * 1024 {
            return Err(input("官方模组文件超出限制"));
        }
        mods.insert(
            path,
            OfficialMod {
                sha1: hash,
                size: file.file_size,
                urls: file.downloads,
                bytes: None,
            },
        );
    }
    // Client overrides take precedence over shared overrides, matching mrpack.
    for prefix in ["overrides/", "client-overrides/"] {
        for index in 0..archive.len() {
            let mut file = archive
                .by_index(index)
                .map_err(|error| input(error.to_string()))?;
            if file.is_dir() {
                continue;
            }
            let Some(name) = file.name().strip_prefix(prefix) else {
                continue;
            };
            let Some(path) = mod_path(name)? else {
                continue;
            };
            if file.size() > 512 * 1024 * 1024 {
                return Err(input("官方模组文件超出限制"));
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            mods.insert(
                path,
                OfficialMod {
                    sha1: Sha1::from(&bytes).hexdigest(),
                    size: bytes.len() as u64,
                    urls: vec![],
                    bytes: Some(bytes),
                },
            );
        }
    }
    // Case collisions must fail even on a case-sensitive development machine.
    let mut names = std::collections::HashSet::new();
    if mods.keys().any(|name| !names.insert(name.to_lowercase())) {
        return Err(input("官方模组路径大小写冲突"));
    }
    Ok(mods)
}

fn reject_link(path: &Path) -> crate::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(input("官方模组目录不能包含链接或目录联接"));
                }
            }
            if metadata.file_type().is_symlink() {
                return Err(input("官方模组目录不能包含符号链接"));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn checked_path(root: &Path, relative: &str) -> crate::Result<PathBuf> {
    reject_link(root)?;
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        path.push(part);
        reject_link(&path)?;
    }
    Ok(path)
}

fn list_files(root: &Path) -> crate::Result<Vec<String>> {
    reject_link(root)?;
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut result = vec![];
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            reject_link(&path)?;
            if path.is_dir() {
                pending.push(path);
            } else {
                result.push(
                    path.strip_prefix(root)
                        .map_err(|_| input("模组路径越界"))?
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    Ok(result)
}

async fn matches(path: &Path, hash: &str, size: u64) -> bool {
    if tokio::fs::metadata(path)
        .await
        .is_ok_and(|meta| meta.is_file() && meta.len() == size)
    {
        sha1_file_async(path)
            .await
            .is_ok_and(|(_, actual)| actual.eq_ignore_ascii_case(hash))
    } else {
        false
    }
}

pub(crate) async fn verify_and_repair(
    instance_id: &str,
    spec: &ManagedInstanceSpec,
) -> crate::Result<()> {
    let state = State::get().await?;
    let digest = spec
        .pack
        .sha1
        .as_deref()
        .ok_or_else(|| input("官方包缺少校验值"))?;
    let size = spec.pack.size.ok_or_else(|| input("官方包缺少大小"))?;
    let pack_path = state
        .directories
        .caches_dir()
        .join("ysumc-packs")
        .join(format!("{}.mrpack", digest.to_lowercase()));
    if !matches(&pack_path, digest, size).await {
        let bytes = fetch(
            &spec.pack.url,
            Some(digest),
            None,
            None,
            &state.fetch_semaphore,
            &state.pool,
        )
        .await?;
        if bytes.len() as u64 != size {
            return Err(input("官方包下载大小不匹配"));
        }
        tokio::fs::create_dir_all(pack_path.parent().unwrap()).await?;
        tokio::fs::write(&pack_path, bytes).await?;
    }
    let parse_path = pack_path.clone();
    let expected =
        tokio::task::spawn_blocking(move || read_official_mods(&parse_path))
            .await
            .map_err(|error| input(error.to_string()))??;
    let game_dir = crate::api::instance::get_full_path(instance_id).await?;
    let mods_dir = game_dir.join("mods");
    let actual = list_files(&mods_dir)?;
    let cache = state.directories.caches_dir().join("ysumc-mods");
    tokio::fs::create_dir_all(&cache).await?;
    let mut repairs = vec![];
    // Prepare all replacements before moving any user file. A network failure
    // therefore leaves the playable directory untouched.
    for (relative, file) in &expected {
        let target = checked_path(&mods_dir, relative)?;
        if matches(&target, &file.sha1, file.size).await {
            continue;
        }
        let source = cache.join(&file.sha1);
        if !matches(&source, &file.sha1, file.size).await {
            let bytes = if let Some(bytes) = &file.bytes {
                bytes.clone()
            } else {
                let mut downloaded = None;
                for url in &file.urls {
                    if !url::Url::parse(url).is_ok_and(|u| {
                        u.scheme() == "https"
                            && u.username().is_empty()
                            && u.password().is_none()
                    }) {
                        continue;
                    }
                    if let Ok(bytes) = fetch(
                        url,
                        Some(&file.sha1),
                        None,
                        None,
                        &state.fetch_semaphore,
                        &state.pool,
                    )
                    .await
                    {
                        if bytes.len() as u64 == file.size {
                            downloaded = Some(bytes.to_vec());
                            break;
                        }
                    }
                }
                downloaded.ok_or_else(|| {
                    input(format!("无法修复官方模组 {relative}，请联网后重试"))
                })?
            };
            tokio::fs::write(&source, bytes).await?;
        }
        repairs.push((relative.clone(), source));
    }
    apply_repairs(&game_dir, &mods_dir, &expected, &actual, repairs).await
}

async fn apply_repairs(
    game_dir: &Path,
    mods_dir: &Path,
    expected: &BTreeMap<String, OfficialMod>,
    actual: &[String],
    repairs: Vec<(String, PathBuf)>,
) -> crate::Result<()> {
    let quarantine_root = game_dir.join("ysumc-mod-quarantine");
    reject_link(&quarantine_root)?;
    let quarantine = quarantine_root.join(uuid::Uuid::new_v4().to_string());
    let repair_names: std::collections::HashSet<_> =
        repairs.iter().map(|(name, _)| name.as_str()).collect();
    for relative in actual {
        if !expected.contains_key(relative)
            || repair_names.contains(relative.as_str())
        {
            let from = checked_path(&mods_dir, relative)?;
            let to = checked_path(&quarantine, relative)?;
            tokio::fs::create_dir_all(to.parent().unwrap()).await?;
            tokio::fs::rename(from, to).await?;
        }
    }
    for (relative, source) in repairs {
        let target = checked_path(&mods_dir, &relative)?;
        tokio::fs::create_dir_all(target.parent().unwrap()).await?;
        tokio::fs::copy(source, &target).await?;
    }
    for (relative, file) in expected {
        if !matches(&checked_path(&mods_dir, &relative)?, &file.sha1, file.size)
            .await
        {
            return Err(input(format!("官方模组校验失败：{relative}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_mods_are_in_scope_and_paths_cannot_escape() {
        assert_eq!(mod_path("config/options.txt").unwrap(), None);
        assert_eq!(mod_path("mods/a.jar").unwrap(), Some("a.jar".into()));
        for path in [
            "mods/../save",
            "mods/a/../../save",
            "mods/C:/secret",
            "mods/a\\b",
            "mods/a./b",
            "mods/NUL.jar",
        ] {
            assert!(mod_path(path).is_err());
        }
    }
    #[tokio::test]
    async fn repairs_and_quarantines_mods_without_touching_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let mods = dir.path().join("mods");
        std::fs::create_dir(&mods).unwrap();
        std::fs::create_dir(dir.path().join("config")).unwrap();
        std::fs::write(dir.path().join("config/options.txt"), b"user settings")
            .unwrap();
        std::fs::write(mods.join("changed.jar"), b"modified").unwrap();
        std::fs::write(mods.join("extra.jar"), b"extra").unwrap();
        std::fs::write(mods.join("kept.jar"), b"official").unwrap();
        let source = dir.path().join("verified-cache");
        std::fs::write(&source, b"official").unwrap();
        let expected = ["changed.jar", "missing.jar", "kept.jar"]
            .into_iter()
            .map(|name| {
                (
                    name.into(),
                    OfficialMod {
                        sha1: Sha1::from(b"official").hexdigest(),
                        size: 8,
                        urls: vec![],
                        bytes: None,
                    },
                )
            })
            .collect();
        apply_repairs(
            dir.path(),
            &mods,
            &expected,
            &list_files(&mods).unwrap(),
            vec![
                ("changed.jar".into(), source.clone()),
                ("missing.jar".into(), source),
            ],
        )
        .await
        .unwrap();
        assert_eq!(list_files(&mods).unwrap().len(), 3);
        assert!(!mods.join("extra.jar").exists());
        assert_eq!(
            std::fs::read(mods.join("changed.jar")).unwrap(),
            b"official"
        );
        assert_eq!(
            std::fs::read(mods.join("missing.jar")).unwrap(),
            b"official"
        );
        assert_eq!(
            std::fs::read(dir.path().join("config/options.txt")).unwrap(),
            b"user settings"
        );
        let quarantine =
            std::fs::read_dir(dir.path().join("ysumc-mod-quarantine"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
        assert_eq!(
            std::fs::read(quarantine.join("changed.jar")).unwrap(),
            b"modified"
        );
        assert_eq!(
            std::fs::read(quarantine.join("extra.jar")).unwrap(),
            b"extra"
        );
    }

    #[test]
    fn client_overrides_win_and_configs_are_excluded() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pack.mrpack");
        let mut zip =
            zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in [
            (
                "modrinth.index.json",
                r#"{"game":"minecraft","formatVersion":1,"versionId":"1","name":"test","files":[],"dependencies":{"minecraft":"1.21.1"}}"#,
            ),
            ("overrides/mods/a.jar", "base"),
            ("client-overrides/mods/a.jar", "client"),
            ("overrides/config/keep.txt", "config"),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let mods = read_official_mods(&path).unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods["a.jar"].bytes.as_deref(), Some(b"client".as_slice()));
    }
}
