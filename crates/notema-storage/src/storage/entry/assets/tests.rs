use super::*;
use tempfile::tempdir;

fn entry_path(root: &Path) -> PathBuf {
    let dir = root.join("work/2026/07/05");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("2026-07-05T14-30-00-abc123.md");
    fs::write(&path, "body").unwrap();
    path
}

fn png_bytes() -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(&[0u8; 16]);
    bytes
}

#[test]
fn cleanup_failure_after_a_durable_write_is_reported_not_returned() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let assets = entry_assets_dir(&entry).unwrap();
    let src = dir.path().join("pic.png");
    fs::write(&src, png_bytes()).unwrap();

    let mut staged = StagedAssets::for_entry(&entry);
    staged
        .ingest(
            &format!("![new]({})", src.display()),
            None,
            EntryAssetOptions::default(),
        )
        .unwrap();

    // Break the sweep without touching what it would have swept: move the
    // folder aside and leave a regular file in its place, so `read_dir` fails.
    let displaced = dir.path().join("displaced.assets");
    fs::rename(&assets, &displaced).unwrap();
    fs::write(&assets, b"not a directory").unwrap();

    let report = staged.commit();

    assert!(
        report.cleanup_failed.is_some(),
        "the sweep failure is reported"
    );
    assert_eq!(
        report.images_not_stored(),
        0,
        "not counted as an image failure"
    );
    assert_eq!(report.attachments_not_stored(), 0);
    assert!(
        !report.is_noop(),
        "a cleanup-only failure is still worth reporting"
    );
    assert_eq!(
        fs::read_dir(&displaced).unwrap().count(),
        1,
        "the staged asset survives a failed sweep"
    );
}

#[test]
fn dropping_uncommitted_staging_removes_only_what_it_staged() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let assets = entry_assets_dir(&entry).unwrap();
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("existing.png"), png_bytes()).unwrap();
    let src = dir.path().join("pic.png");
    fs::write(&src, png_bytes()).unwrap();

    let mut staged = StagedAssets::for_entry(&entry);
    let body = format!(
        "![keep](2026-07-05T14-30-00-abc123.assets/existing.png)\n![new]({})",
        src.display()
    );
    let rewritten = staged
        .ingest(&body, None, EntryAssetOptions::default())
        .unwrap()
        .unwrap();
    assert!(rewritten.contains("existing.png"), "kept reference intact");
    assert_eq!(
        fs::read_dir(&assets).unwrap().count(),
        2,
        "ingest wrote one"
    );

    drop(staged);

    let remaining: Vec<_> = fs::read_dir(&assets)
        .unwrap()
        .map(|item| item.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        remaining,
        ["existing.png"],
        "only the staged file rolled back"
    );
}

#[test]
fn dropping_uncommitted_staging_removes_a_directory_it_created() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let assets = entry_assets_dir(&entry).unwrap();
    let src = dir.path().join("pic.png");
    fs::write(&src, png_bytes()).unwrap();

    let mut staged = StagedAssets::for_entry(&entry);
    staged
        .ingest(
            &format!("![new]({})", src.display()),
            None,
            EntryAssetOptions::default(),
        )
        .unwrap();
    assert!(assets.exists(), "ingest created the folder");

    drop(staged);

    assert!(
        !assets.exists(),
        "the folder this operation created is gone"
    );
}

#[test]
fn dropping_uncommitted_staging_keeps_a_directory_it_did_not_create() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let assets = entry_assets_dir(&entry).unwrap();
    fs::create_dir_all(&assets).unwrap();
    let src = dir.path().join("pic.png");
    fs::write(&src, png_bytes()).unwrap();

    let mut staged = StagedAssets::for_entry(&entry);
    staged
        .ingest(
            &format!("![new]({})", src.display()),
            None,
            EntryAssetOptions::default(),
        )
        .unwrap();

    drop(staged);

    assert!(
        assets.exists(),
        "a pre-existing folder is not this operation's to remove"
    );
    assert_eq!(fs::read_dir(&assets).unwrap().count(), 0);
}

#[test]
fn cleanup_removes_asset_when_reference_dropped() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let src = dir.path().join("pic.png");
    fs::write(&src, png_bytes()).unwrap();

    let body = format!("![shot]({})", src.display());
    let (new_body, _) = ingest_and_cleanup(&entry, &body, None, true).unwrap();
    let new_body = new_body.unwrap();
    let assets = entry_assets_dir(&entry).unwrap();

    // Re-running with the reference still present keeps the asset.
    let (_, report) = ingest_and_cleanup(&entry, &new_body, None, true).unwrap();
    assert_eq!(report.removed, 0);

    // Dropping the reference removes the asset and prunes the empty dir.
    let (_, report) = ingest_and_cleanup(&entry, "no images", None, true).unwrap();
    assert_eq!(report.removed, 1, "original removed");
    assert!(!assets.exists(), "empty folder pruned");
}

#[test]
fn ingests_local_markdown_image_and_rewrites_ref() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let src = dir.path().join("pic.png");
    fs::write(&src, png_bytes()).unwrap();

    let body = format!("Look:\n![a shot]({})\nend", src.display());
    let (new_body, report) = ingest_and_cleanup(&entry, &body, None, true).unwrap();

    let new_body = new_body.expect("body should change");
    assert_eq!(report.stored, 1);
    assert_eq!(report.images_stored(), 1);
    assert!(new_body.contains("![a shot](2026-07-05T14-30-00-abc123.assets/"));
    let assets = entry_assets_dir(&entry).unwrap();
    let files: Vec<_> = fs::read_dir(&assets).unwrap().collect();
    assert_eq!(files.len(), 1);
}

#[test]
fn wraps_bare_image_path_line() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let src = dir.path().join("bare.png");
    fs::write(&src, png_bytes()).unwrap();

    let body = format!("{}", src.display());
    let (new_body, report) = ingest_and_cleanup(&entry, &body, None, true).unwrap();

    let new_body = new_body.expect("body should change");
    assert_eq!(report.stored, 1);
    assert_eq!(report.images_stored(), 1);
    assert!(new_body.starts_with("![](2026-07-05T14-30-00-abc123.assets/"));
}

#[test]
fn wraps_bare_image_path_line_with_spaces() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let src = dir.path().join("My Photo.png");
    fs::write(&src, png_bytes()).unwrap();

    let body = format!("{}", src.display());
    let (new_body, report) = ingest_and_cleanup(&entry, &body, None, true).unwrap();

    let new_body = new_body.expect("body should change");
    assert_eq!(report.stored, 1);
    assert_eq!(report.images_stored(), 1);
    assert!(new_body.starts_with("![](2026-07-05T14-30-00-abc123.assets/"));
}

#[test]
fn wraps_bare_image_path_line_with_escaped_space() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let src = dir.path().join("My Photo.png");
    fs::write(&src, png_bytes()).unwrap();

    // A path dragged/pasted into a terminal escapes the space: `My\ Photo`.
    let body = src.display().to_string().replace(' ', "\\ ");
    assert!(body.contains("\\ "), "test setup should contain an escape");
    let (new_body, report) = ingest_and_cleanup(&entry, &body, None, true).unwrap();

    let new_body = new_body.expect("body should change");
    assert_eq!(report.stored, 1);
    assert_eq!(report.images_stored(), 1);
    assert!(new_body.starts_with("![](2026-07-05T14-30-00-abc123.assets/"));
}

#[test]
fn unescape_shell_path_handles_escapes_and_quotes() {
    assert_eq!(unescape_shell_path("/a/IMG\\ 2.jpeg"), "/a/IMG 2.jpeg");
    assert_eq!(unescape_shell_path("'/a/My Photo.png'"), "/a/My Photo.png");
    assert_eq!(
        unescape_shell_path("\"/a/My Photo.png\""),
        "/a/My Photo.png"
    );
    assert_eq!(unescape_shell_path("/a/plain.png"), "/a/plain.png");
    assert_eq!(unescape_shell_path("/a/b\\(1\\).png"), "/a/b(1).png");
}

#[test]
fn leaves_prose_with_image_extension_untouched() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());

    // A line ending in an image extension but not a real file is left alone.
    let (changed, report) =
        ingest_and_cleanup(&entry, "here is my summary.png", None, true).unwrap();

    assert!(changed.is_none());
    assert!(report.is_noop());
}

#[test]
fn cleanup_deletes_unreferenced_asset() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let assets = entry_assets_dir(&entry).unwrap();
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("zz.png"), png_bytes()).unwrap();

    // Body references nothing in the folder → the orphan is removed.
    let (changed, report) = ingest_and_cleanup(&entry, "no images here", None, true).unwrap();

    assert!(changed.is_none());
    assert_eq!(report.removed, 1);
    assert!(!assets.exists(), "empty folder should be removed");
}

#[test]
fn keeps_referenced_asset() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let assets = entry_assets_dir(&entry).unwrap();
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("zz.png"), png_bytes()).unwrap();

    let body = "![](2026-07-05T14-30-00-abc123.assets/zz.png)";
    let (changed, report) = ingest_and_cleanup(&entry, body, None, true).unwrap();

    assert!(changed.is_none());
    assert_eq!(report.removed, 0);
    assert!(assets.join("zz.png").exists());
}

#[test]
fn keeps_reference_style_and_html_linked_assets() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let assets = entry_assets_dir(&entry).unwrap();
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("ref.png"), png_bytes()).unwrap();
    fs::write(assets.join("html.png"), png_bytes()).unwrap();

    // Neither form is a canonical inline embed, but both point at stored
    // assets and must survive the sweep.
    let body = "![alt][a]\n\n[a]: 2026-07-05T14-30-00-abc123.assets/ref.png\n\n\
         <img src=\"2026-07-05T14-30-00-abc123.assets/html.png\">";
    let (changed, report) = ingest_and_cleanup(&entry, body, None, true).unwrap();

    assert!(changed.is_none());
    assert_eq!(report.removed, 0, "hand-written links are not orphaned");
    assert!(assets.join("ref.png").exists());
    assert!(assets.join("html.png").exists());
}

#[test]
fn ingests_local_file_attachment_link_and_rewrites_ref() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let src = dir.path().join("report.pdf");
    fs::write(&src, b"%PDF-1.4 data").unwrap();

    let body = format!("See [PDF attachment]({})", src.display());
    let (new_body, report) = ingest_and_cleanup(&entry, &body, None, true).unwrap();

    let new_body = new_body.expect("body should change");
    assert_eq!(report.stored, 1);
    assert_eq!(report.attachments_stored, 1);
    assert!(
        new_body.starts_with("See [PDF attachment](2026-07-05T14-30-00-abc123.assets/"),
        "link rewritten to canonical: {new_body}"
    );
    assert!(
        new_body.ends_with(".pdf)"),
        "extension preserved: {new_body}"
    );
    let assets = entry_assets_dir(&entry).unwrap();
    assert_eq!(fs::read_dir(&assets).unwrap().count(), 1);
}

#[test]
fn cleanup_keeps_attachment_link_and_removes_unreferenced() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let src = dir.path().join("clip.mp4");
    fs::write(&src, b"video bytes").unwrap();

    let body = format!("[Video attachment]({})", src.display());
    let (stored_body, _) = ingest_and_cleanup(&entry, &body, None, true).unwrap();
    let stored_body = stored_body.unwrap();

    // Re-running with the link present keeps the attachment.
    let (_, report) = ingest_and_cleanup(&entry, &stored_body, None, true).unwrap();
    assert_eq!(report.removed, 0);

    // Dropping the link prunes the attachment.
    let (_, report) = ingest_and_cleanup(&entry, "no links", None, true).unwrap();
    assert_eq!(report.removed, 1);
}

#[test]
fn wraps_bare_non_image_path_line_as_attachment() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let src = dir.path().join("My Report.pdf");
    fs::write(&src, b"%PDF bare").unwrap();

    let body = src.display().to_string();
    let (new_body, report) = ingest_and_cleanup(&entry, &body, None, true).unwrap();

    let new_body = new_body.expect("body should change");
    assert_eq!(report.stored, 1);
    assert_eq!(report.attachments_stored, 1);
    assert!(
        new_body.starts_with("[My Report.pdf](2026-07-05T14-30-00-abc123.assets/"),
        "labelled by file name: {new_body}"
    );
    assert!(new_body.ends_with(".pdf)"), "{new_body}");
}

#[test]
fn leaves_url_and_anchor_links_untouched() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());

    let body = "[docs](https://example.com/x.pdf) and [top](#heading)";
    let (changed, report) = ingest_and_cleanup(&entry, body, None, true).unwrap();

    assert!(changed.is_none());
    assert!(report.is_noop());
}

#[test]
fn encrypted_attachment_is_written_as_age_and_resolves() {
    let dir = tempdir().unwrap();
    let entry = dir
        .path()
        .join("work/2026/07/05/2026-07-05T14-30-00-abc123.md.age");
    fs::create_dir_all(entry.parent().unwrap()).unwrap();
    let paths = KeyPaths::for_config(
        &dir.path().join("config.toml"),
        &dir.path().join("journals"),
    )
    .unwrap();
    crypto::initialize_store_identity(&paths, "laptop", Some(&crate::SecretString::from("secret")))
        .unwrap();

    let src = dir.path().join("notes.pdf");
    fs::write(&src, b"%PDF secret").unwrap();

    let body = format!("[PDF attachment]({})", src.display());
    let (new_body, report) = ingest_and_cleanup(&entry, &body, Some(&paths), true).unwrap();

    let new_body = new_body.expect("body should change");
    assert_eq!(report.stored, 1);
    assert_eq!(report.attachments_stored, 1);
    // The body link stays clean; only the on-disk file carries `.age`.
    assert!(new_body.contains(".pdf)") && !new_body.contains(".age"));
    let assets = entry_assets_dir(&entry).unwrap();
    let stored = fs::read_dir(&assets)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(stored.to_string_lossy().ends_with(".pdf.age"));

    // The clean link resolves to the encrypted file on disk.
    let file_name = new_body
        .rsplit_once('/')
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .unwrap();
    let resolved = resolve_entry_asset_path(&entry, file_name)
        .unwrap()
        .unwrap();
    assert_eq!(resolved, fs::canonicalize(&stored).unwrap());
    let identity =
        crypto::unlock_identity(&paths, Some(&crate::SecretString::from("secret"))).unwrap();
    let decrypted = crypto::decrypt_file_bytes(&identity, &resolved).unwrap();
    assert_eq!(decrypted.as_bytes(), b"%PDF secret");
}

#[test]
fn report_separates_image_and_attachment_outcomes() {
    let report = AssetReport {
        stored: 3,
        attachments_stored: 1,
        failed: vec![
            AssetFailure::RemoteUnavailable {
                source: "https://example.com/image.png".to_string(),
            },
            AssetFailure::Ingest {
                source: "image.png".to_string(),
                error: "bad image".to_string(),
            },
            AssetFailure::AttachmentIngest {
                source: "recording.m4a".to_string(),
                error: "gone".to_string(),
            },
        ],
        ..AssetReport::default()
    };

    assert_eq!(report.images_stored(), 2);
    assert_eq!(report.images_not_stored(), 2);
    assert_eq!(report.attachments_not_stored(), 1);
}

#[test]
fn leaves_internal_ref_untouched() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let assets = entry_assets_dir(&entry).unwrap();
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("zz.png"), png_bytes()).unwrap();

    let body = "![alt](2026-07-05T14-30-00-abc123.assets/zz.png)";
    let (changed, report) = ingest_and_cleanup(&entry, body, None, true).unwrap();

    assert!(changed.is_none());
    assert!(report.is_noop());
}

#[test]
fn duplicate_source_is_stored_once_and_reused() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let src = dir.path().join("pic.png");
    fs::write(&src, png_bytes()).unwrap();

    let body = format!("![one]({})\n![two]({})", src.display(), src.display());
    let (new_body, report) = ingest_and_cleanup(&entry, &body, None, true).unwrap();

    let new_body = new_body.expect("body should change");
    assert_eq!(report.stored, 1);
    let assets = entry_assets_dir(&entry).unwrap();
    let files: Vec<_> = fs::read_dir(&assets).unwrap().collect();
    assert_eq!(files.len(), 1);
    let links: Vec<_> = new_body.lines().collect();
    assert_eq!(links.len(), 2);
    assert_ne!(links[0], links[1], "alt text differs");
    let first_target = links[0].split('(').nth(1).unwrap();
    let second_target = links[1].split('(').nth(1).unwrap();
    assert_eq!(first_target, second_target);
}

#[test]
fn stored_reference_accepts_only_exact_entry_asset_file() {
    let dir_name = "2026-07-05T14-30-00-abc123.assets";

    let reference = stored_asset_reference(&format!("{dir_name}/x9k2.png"), dir_name)
        .expect("canonical reference should parse");
    assert_eq!(reference.file_name, "x9k2.png");

    assert!(stored_asset_reference("../x9k2.png", dir_name).is_none());
    assert!(stored_asset_reference(&format!("{dir_name}/../x9k2.png"), dir_name).is_none());
    assert!(stored_asset_reference(&format!("{dir_name}/nested/x9k2.png"), dir_name).is_none());
    assert!(stored_asset_reference("/tmp/x9k2.png", dir_name).is_none());
    assert!(stored_asset_reference("https://example.com/x9k2.png", dir_name).is_none());
    assert!(
        stored_asset_reference("2026-07-05T14-30-00-other.assets/x9k2.png", dir_name).is_none()
    );
}

#[test]
fn retarget_stored_links_changes_only_markdown_asset_targets() {
    let source = "old.assets";
    let target = "new.assets";
    let body = concat!(
        "![photo](old.assets/x9k2.png)\n",
        "[recording](old.assets/a1.m4a)\n",
        "ordinary old.assets/x9k2.png text\n",
        "```markdown\n",
        "![example](old.assets/x9k2.png)\n",
        "```\n",
        "![other](different.assets/x9k2.png)\n",
    );

    let rewritten = retarget_stored_asset_links(body, source, target);

    assert_eq!(
        rewritten,
        concat!(
            "![photo](new.assets/x9k2.png)\n",
            "[recording](new.assets/a1.m4a)\n",
            "ordinary old.assets/x9k2.png text\n",
            "```markdown\n",
            "![example](old.assets/x9k2.png)\n",
            "```\n",
            "![other](different.assets/x9k2.png)\n",
        )
    );
}

#[test]
fn sole_stored_image_matches_in_folder_line() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());

    let image = sole_stored_image(
        "![a shot](2026-07-05T14-30-00-abc123.assets/x9k2.png)",
        &entry,
    )
    .expect("should match");
    assert_eq!(image.alt, "a shot");
    assert_eq!(image.file_name, "x9k2.png");
    assert_eq!(image.link, None);

    // Leading/trailing whitespace around the sole image is ignored.
    assert!(
        sole_stored_image(
            "   ![](2026-07-05T14-30-00-abc123.assets/x9k2.png)  ",
            &entry
        )
        .is_some()
    );
}

#[test]
fn sole_stored_image_rejects_external_wrong_folder_and_traversal() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());

    assert!(sole_stored_image("![](https://example.com/x.png)", &entry).is_none());
    assert!(sole_stored_image("![](/etc/x.png)", &entry).is_none());
    assert!(sole_stored_image("![](other/x9k2.png)", &entry).is_none());
    assert!(sole_stored_image("![](2026-07-05T14-30-00-other.assets/x9k2.png)", &entry).is_none());
    assert!(
        sole_stored_image("![](2026-07-05T14-30-00-abc123.assets/../x9k2.png)", &entry).is_none()
    );
}

#[test]
fn sole_stored_image_rejects_extra_text_or_multiple_images() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let asset = "2026-07-05T14-30-00-abc123.assets/x9k2.png";

    assert!(sole_stored_image(&format!("look ![]({asset})"), &entry).is_none());
    assert!(sole_stored_image(&format!("![]({asset}) trailing"), &entry).is_none());
    assert!(sole_stored_image(&format!("![]({asset}) ![]({asset})"), &entry).is_none());
}

#[test]
fn sole_stored_image_matches_a_link_wrapped_image() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let asset = "2026-07-05T14-30-00-abc123.assets/x9k2.png";

    let image = sole_stored_image(
        &format!("[![a shot]({asset})](https://example.org/page)"),
        &entry,
    )
    .expect("should match");
    assert_eq!(image.alt, "a shot");
    assert_eq!(image.file_name, "x9k2.png");
    assert_eq!(image.link.as_deref(), Some("https://example.org/page"));

    // The surrounding whitespace trim covers the wrapped shape too.
    let padded =
        sole_stored_image(&format!("  [![]({asset})](#notes)  "), &entry).expect("should match");
    assert_eq!(padded.link.as_deref(), Some("#notes"));
}

#[test]
fn sole_stored_image_keeps_a_wrapper_with_no_target() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let asset = "2026-07-05T14-30-00-abc123.assets/x9k2.png";

    // The image still earns its label; the empty href simply opens nothing.
    let image = sole_stored_image(&format!("[![]({asset})]()"), &entry).expect("should match");
    assert_eq!(image.link.as_deref(), Some(""));
}

#[test]
fn sole_stored_image_rejects_a_loose_wrapper() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let asset = "2026-07-05T14-30-00-abc123.assets/x9k2.png";

    for line in [
        format!("see [![]({asset})](https://x)"),
        format!("[![]({asset})](https://x) trailing"),
        format!("[text ![]({asset})](https://x)"),
        format!("![]({asset})](https://x)"),
        format!("[![]({asset})]"),
        format!("[![]({asset})](https://x \"Title\")"),
        format!("[![]({asset})](https://x/(y))"),
        format!("[![]({asset})](https://x) ![]({asset})"),
    ] {
        assert!(
            sole_stored_image(&line, &entry).is_none(),
            "should reject: {line}"
        );
    }
}

#[test]
fn resolve_entry_asset_path_rejects_traversal_and_wrong_folder() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let assets = entry_assets_dir(&entry).unwrap();
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("x9k2.png"), png_bytes()).unwrap();

    assert!(
        resolve_entry_asset_path(&entry, "x9k2.png")
            .unwrap()
            .is_some()
    );
    assert!(
        resolve_entry_asset_path(&entry, "../x9k2.png")
            .unwrap()
            .is_none()
    );
    assert!(
        resolve_entry_asset_path(&entry, "nested/x9k2.png")
            .unwrap()
            .is_none()
    );
}

#[cfg(unix)]
#[test]
fn resolve_entry_asset_path_rejects_symlink() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let assets = entry_assets_dir(&entry).unwrap();
    fs::create_dir_all(&assets).unwrap();
    let outside = dir.path().join("outside.png");
    fs::write(&outside, png_bytes()).unwrap();
    symlink(&outside, assets.join("linked.png")).unwrap();

    assert!(
        resolve_entry_asset_path(&entry, "linked.png")
            .unwrap()
            .is_none()
    );
}

#[test]
fn skips_remote_when_downloads_disabled() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());

    let body = "![](https://example.com/pic.png)";
    let (changed, report) = ingest_and_cleanup(&entry, body, None, false).unwrap();

    assert!(changed.is_none());
    assert_eq!(report.stored, 0);
    assert_eq!(
        report.failed,
        vec![AssetFailure::RemoteUnavailable {
            source: "https://example.com/pic.png".to_string(),
        }]
    );
}

#[test]
fn encrypted_asset_is_written_as_age_and_round_trips() {
    let dir = tempdir().unwrap();
    let entry = dir
        .path()
        .join("work/2026/07/05/2026-07-05T14-30-00-abc123.md.age");
    fs::create_dir_all(entry.parent().unwrap()).unwrap();
    let paths = KeyPaths::for_config(
        &dir.path().join("config.toml"),
        &dir.path().join("journals"),
    )
    .unwrap();
    crypto::initialize_store_identity(&paths, "laptop", Some(&crate::SecretString::from("secret")))
        .unwrap();
    let identity =
        crypto::unlock_identity(&paths, Some(&crate::SecretString::from("secret"))).unwrap();

    let src = dir.path().join("pic.png");
    let original = png_bytes();
    fs::write(&src, &original).unwrap();

    let body = format!("![shot]({})", src.display());
    let (new_body, report) = ingest_and_cleanup(&entry, &body, Some(&paths), true).unwrap();

    let new_body = new_body.expect("body should change");
    assert_eq!(report.stored, 1);
    // The body link stays clean (no `.age`) even though the store is encrypted;
    // only the file on disk carries the `.age` suffix.
    assert!(
        new_body.contains(".png)") && !new_body.contains(".age"),
        "link should stay clean: {new_body}"
    );
    let assets = entry_assets_dir(&entry).unwrap();
    let stored = fs::read_dir(&assets)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(stored.to_string_lossy().ends_with(".png.age"));
    let decrypted = crypto::decrypt_file_bytes(&identity, &stored).unwrap();
    assert_eq!(decrypted.as_bytes(), original);

    // The clean link resolves to the encrypted file on disk.
    let file_name = new_body
        .rsplit_once('/')
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .unwrap();
    let resolved = resolve_entry_asset_path(&entry, file_name)
        .unwrap()
        .unwrap();
    assert_eq!(resolved, fs::canonicalize(&stored).unwrap());
}

#[test]
fn ignores_image_inside_code_fence() {
    let dir = tempdir().unwrap();
    let entry = entry_path(dir.path());
    let src = dir.path().join("pic.png");
    fs::write(&src, png_bytes()).unwrap();

    let body = format!("```\n![x]({})\n```", src.display());
    let (changed, report) = ingest_and_cleanup(&entry, &body, None, true).unwrap();

    assert!(changed.is_none());
    assert!(report.is_noop());
}
