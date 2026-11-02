"""Package verified, unmodified publisher MSIX files for external distribution.

No downloads or installation: the publisher packages are supplied explicitly.
The generated ZIP includes the local Windows installation helper and license.
"""
import argparse
import hashlib
import json
import shutil
import subprocess
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

SOURCES = {
    "codex": {
        "name": "Codex 桌面版",
        "file": "Codex.msix",
        "identity": "OpenAI.Codex",
        "sha256": "d442a3d7d5c229f45e5e11995711ac2619c8dbc89e3fa2bc185287dc8db8e76e",
        "source": "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-x64.msix",
        "documentation": "https://learn.chatgpt.com/docs/enterprise/windows-deployment",
        "license": "ChatGPT-License.xml",
        "licenseSha256": "c26569dbc30c1f630d49a9bd305b6496b1a467319f495b21d1fc7919d3c0481a",
    },
    "claude-desktop": {
        "name": "Claude Desktop",
        "file": "Claude.msix",
        "identity": "Claude",
        "sha256": "5e7cd0edfe4a5561e6f69b0249757d23de752a74151e016fe81bc939ddc5439e",
        "source": "https://downloads.claude.ai/releases/win32/x64/4.4212.0/Claude-cbfe3c56d49c49332ad5a9588303c9dbc70a5e7f.msix",
        "documentation": "https://support.claude.com/en/articles/12622703-deploy-claude-desktop-for-windows",
    },
}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def build(source_dir, output_dir, helpers, makensis=None):
    output_dir.mkdir(parents=True, exist_ok=True)
    entries = []
    for app, source in SOURCES.items():
        msix = source_dir / source["file"]
        if digest(msix) != source["sha256"]:
            raise ValueError(f"Unexpected publisher package: {msix}")
        with zipfile.ZipFile(msix) as archive:
            root = ET.fromstring(archive.read("AppxManifest.xml"))
            identity = root.find("{http://schemas.microsoft.com/appx/manifest/foundation/windows10}Identity").attrib
            if identity["Name"] != source["identity"] or identity["ProcessorArchitecture"] != "x64":
                raise ValueError(f"Wrong package identity or architecture: {identity}")
            if "AppxSignature.p7x" not in archive.namelist():
                raise ValueError("Publisher signature missing")
        files = [{"name": msix.name, "sha256": source["sha256"]}]
        if source.get("license"):
            license_file = source_dir / source["license"]
            if digest(license_file) != source["licenseSha256"]:
                raise ValueError("Unexpected offline license")
            files.append({"name": license_file.name, "sha256": source["licenseSha256"]})
        manifest = {
            "name": source["name"], "version": identity["Version"],
            "identity": identity["Name"], "architecture": "x64", "package": msix.name,
            "license": source.get("license"), "source": source["source"],
            "documentation": source["documentation"], "files": files,
        }
        zip_name = f"{app}-windows-x64-offline.zip"
        target = output_dir / zip_name
        # MSIX files are already compressed; keep the signed publisher bytes intact.
        with zipfile.ZipFile(target.with_suffix(".zip.part"), "w", zipfile.ZIP_STORED, allowZip64=True) as archive:
            for item in files:
                archive.write(source_dir / item["name"], item["name"])
            archive.write(helpers / "Install.cmd", "Install.cmd")
            # Windows PowerShell 5.1 needs a BOM for non-ASCII paths and messages.
            archive.writestr("install-desktop.ps1", (helpers / "install-desktop.ps1").read_text(encoding="utf-8-sig").encode("utf-8-sig"))
            archive.writestr("package.json", json.dumps(manifest, ensure_ascii=False, indent=2).encode("utf-8-sig"))
            note = "\n官方新版安装后显示为 ChatGPT，包含 Codex。\n" if app == "codex" else ""
            archive.writestr("安装说明.txt", (
                f"{source['name']} · Windows x64 离线安装\n\n"
                "1. 将 ZIP 完整解压到一个文件夹。\n"
                "2. 双击 Install.cmd，在 Windows 权限提示中选择允许。\n"
                "3. 安装完成后返回 Oceanway Config，点击“重新检测”。\n"
                f"{note}\n适用 Windows 10 2004 及以上版本。安装脚本校验原厂签名与文件哈希；安装过程使用本地文件。\n"
                "已有更高版本时保留现有版本；不会删除应用设置和聊天历史。运行中的应用可能需要手动关闭后重试。\n"
                "离线安装仅指安装文件齐全；使用 AI 仍需连接 Oceanway API。\n"
                f"\n官方说明：{source['documentation']}\n"
            ).encode("utf-8-sig"))
        target.with_suffix(".zip.part").replace(target)
        if makensis:
            # The self-extracting wrapper keeps publisher-signed MSIX bytes intact.
            payload = output_dir.parent / "payloads" / app
            payload.mkdir(parents=True, exist_ok=True)
            with zipfile.ZipFile(target) as archive:
                archive.extractall(payload)
            target = output_dir / f"{app}-windows-x64-offline.exe"
            subprocess.run([makensis, "-V2", f"-DAPP_NAME={source['name']}",
                            f"-DPAYLOAD_DIR={payload.resolve()}", f"-DOUTPUT_FILE={target.resolve()}",
                            str(helpers / "desktop-installer.nsi")], check=True)
        entries.append({"app": app, **manifest, "platform": "windows", "minimumOS": "Windows 10 2004+", "filename": target.name, "size": target.stat().st_size, "sha256": digest(target)})
    (output_dir / "manifest.json").write_text(json.dumps({"schemaVersion": 1, "packages": entries}, ensure_ascii=False, indent=2), encoding="utf-8")
    (output_dir / "SHA256SUMS.txt").write_text("".join(f"{e['sha256']}  {e['filename']}\n" for e in entries), encoding="ascii")
    shutil.copyfile(helpers / "index.html", output_dir / "index.html")
    return entries


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--makensis")
    args = parser.parse_args()
    print(json.dumps(build(args.source, args.output, Path(__file__).parent, args.makensis), ensure_ascii=False, indent=2))
