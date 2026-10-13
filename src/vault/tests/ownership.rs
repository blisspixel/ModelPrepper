use super::*;

#[test]
fn closing_catalog_releases_ownership_despite_an_inherited_descriptor() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let config = Config::default_for(root.join("catalog"), root.join("models"));
    initialize(&config).unwrap();
    let catalog = open_catalog(&config, false).unwrap();
    let inherited = catalog._writer.0.try_clone().unwrap();
    drop(catalog);
    let reopened = open_catalog(&config, false).unwrap();
    assert!(inherited.try_lock().is_err());
    drop(reopened);
    inherited.try_lock().unwrap();
}
