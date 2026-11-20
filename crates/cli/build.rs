fn main() {
    println!(
        "cargo:rustc-env=MRIMPORT_TARGET={}",
        std::env::var("TARGET").unwrap()
    );

    // Windows: embed version info, an icon and an asInvoker manifest. An exe
    // with none of these is a common trigger for Defender's machine-learning
    // detections (e.g. "Wacatac"), which quarantined the unsigned CLI.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=../../src-tauri/icons/icon.ico");
        let mut res = tauri_winres::WindowsResource::new();
        res.set_icon("../../src-tauri/icons/icon.ico")
            .set("ProductName", "Modrinth Importer")
            .set("FileDescription", "Modrinth Importer command-line tool")
            .set("CompanyName", "Aoof")
            .set("LegalCopyright", "Copyright (c) Aoof. MIT License.")
            .set("OriginalFilename", "mrimport.exe")
            .set("InternalName", "mrimport")
            .set_manifest(MANIFEST);
        res.compile().expect("failed to embed Windows resources");
    }
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="Aoof.ModrinthImporter.mrimport" version="1.0.0.0"/>
  <description>Modrinth Importer command-line tool</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges>
      <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
    </requestedPrivileges></security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
</assembly>
"#;
