#!/usr/bin/env python3
"""
One-click Codex third-party model provider setup.

This script updates ~/.codex/config.toml and ~/.codex/auth.json using the same
Codex provider shape used by cc-switch.
"""

from __future__ import annotations

import argparse
import getpass
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Iterable

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    tomllib = None


RESERVED_PROVIDER_IDS = {"openai", "ollama", "lmstudio"}
VALID_PROVIDER_ID = re.compile(r"^[A-Za-z0-9_-]+$")
TABLE_RE = re.compile(r"^\s*\[\[?\s*([^\]]+?)\s*\]?\]\s*(?:#.*)?$")
ROOT_MODEL_RE = re.compile(r'^\s*model\s*=\s*"([^"]+)"\s*(?:#.*)?$')
GUI_PROVIDER_ID = "OceanWay"
GUI_PROVIDER_NAME = "OceanWay"
GUI_BASE_URL = "https://ocean-way.top"
GUI_MODEL_FALLBACK = "gpt-5.4"
CODEX_AUTH_KEY = "OPENAI_API_KEY"


@dataclass(frozen=True)
class Preset:
    provider_id: str
    name: str
    base_url: str
    env_key: str
    model: str = ""
    query_params: tuple[tuple[str, str], ...] = ()


PRESETS: dict[str, Preset] = {
    "openrouter": Preset(
        "openrouter",
        "OpenRouter",
        "https://openrouter.ai/api/v1",
        "OPENROUTER_API_KEY",
    ),
    "deepseek": Preset(
        "deepseek",
        "DeepSeek",
        "https://api.deepseek.com/v1",
        "DEEPSEEK_API_KEY",
        "deepseek-chat",
    ),
    "moonshot": Preset(
        "moonshot",
        "Moonshot AI",
        "https://api.moonshot.cn/v1",
        "MOONSHOT_API_KEY",
    ),
    "siliconflow": Preset(
        "siliconflow",
        "SiliconFlow",
        "https://api.siliconflow.cn/v1",
        "SILICONFLOW_API_KEY",
    ),
    "dashscope": Preset(
        "dashscope",
        "Alibaba DashScope",
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "DASHSCOPE_API_KEY",
    ),
    "volcengine": Preset(
        "volcengine",
        "Volcengine Ark",
        "https://ark.cn-beijing.volces.com/api/v3",
        "ARK_API_KEY",
    ),
    "azure": Preset(
        "azure",
        "Azure OpenAI",
        "",
        "AZURE_OPENAI_API_KEY",
        "",
        (("api-version", "2025-04-01-preview"),),
    ),
    "custom": Preset("custom", "Custom Provider", "", "CUSTOM_PROVIDER_API_KEY"),
}


def toml_string(value: str) -> str:
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    escaped = escaped.replace("\n", "\\n").replace("\r", "\\r")
    return f'"{escaped}"'


def toml_inline_map(items: Iterable[tuple[str, str]]) -> str:
    pairs = [f"{toml_string(k)} = {toml_string(v)}" for k, v in items if k and v]
    return "{ " + ", ".join(pairs) + " }"


def shell_single_quote(value: str) -> str:
    return "'" + value.replace("'", "'\"'\"'") + "'"


def powershell_single_quote(value: str) -> str:
    return "'" + value.replace("'", "''") + "'"


def is_windows() -> bool:
    return platform.system() == "Windows"


def default_shell_profile() -> Path:
    if is_windows():
        return Path.home() / "Documents" / "PowerShell" / "Microsoft.PowerShell_profile.ps1"
    return Path.home() / ".zshrc"


def default_env_file(codex_home: Path) -> Path:
    suffix = "ps1" if is_windows() else "sh"
    return codex_home / f"provider-env.{suffix}"


def prompt(default: str, message: str, *, required: bool = True) -> str:
    suffix = f" [{default}]" if default else ""
    while True:
        value = input(f"{message}{suffix}: ").strip()
        if not value:
            value = default
        if value or not required:
            return value
        print("This value is required.")


def choose_preset() -> Preset:
    names = list(PRESETS)
    print("Select a provider preset:")
    for idx, key in enumerate(names, start=1):
        preset = PRESETS[key]
        print(f"  {idx}. {key:<12} {preset.name}")
    while True:
        raw = input("Provider number or id [openrouter]: ").strip() or "openrouter"
        if raw.isdigit() and 1 <= int(raw) <= len(names):
            return PRESETS[names[int(raw) - 1]]
        if raw in PRESETS:
            return PRESETS[raw]
        print("Unknown provider. Run with --list to see supported presets.")


def parse_pairs(raw_values: list[str], label: str) -> list[tuple[str, str]]:
    pairs: list[tuple[str, str]] = []
    for raw in raw_values:
        if "=" not in raw:
            raise SystemExit(f"{label} must use KEY=VALUE: {raw}")
        key, value = raw.split("=", 1)
        key = key.strip()
        value = value.strip()
        if not key or not value:
            raise SystemExit(f"{label} must use non-empty KEY=VALUE: {raw}")
        pairs.append((key, value))
    return pairs


def split_root(lines: list[str]) -> tuple[list[str], list[str]]:
    for idx, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith("[") and not stripped.startswith("#"):
            return lines[:idx], lines[idx:]
    return lines, []


def set_root_key(lines: list[str], key: str, value: str) -> list[str]:
    rendered = f"{key} = {toml_string(value)}\n"
    pattern = re.compile(rf"^\s*{re.escape(key)}\s*=")
    for idx, line in enumerate(lines):
        if not line.lstrip().startswith("#") and pattern.match(line):
            lines[idx] = rendered
            return lines

    insert_at = len(lines)
    while insert_at > 0 and lines[insert_at - 1].strip() == "":
        insert_at -= 1
    lines.insert(insert_at, rendered)
    if insert_at == len(lines) - 1:
        lines.append("\n")
    return lines


def table_path(line: str) -> str | None:
    if line.lstrip().startswith("#"):
        return None
    match = TABLE_RE.match(line)
    return match.group(1).strip() if match else None


def remove_table_family(lines: list[str], root_path: str) -> list[str]:
    output: list[str] = []
    skipping = False
    for line in lines:
        path = table_path(line)
        if path is not None:
            normalized = path.replace('"', "")
            skipping = normalized == root_path or normalized.startswith(root_path + ".")
        if not skipping:
            output.append(line)
    return output


def render_provider_block(
    provider: Preset,
    headers: list[tuple[str, str]],
    env_headers: list[tuple[str, str]],
    extra_query_params: list[tuple[str, str]],
) -> list[str]:
    query_params = list(provider.query_params) + extra_query_params
    block = [
        "\n",
        "# Managed by setup-codex-provider.py\n",
        f"[model_providers.{provider.provider_id}]\n",
        f"name = {toml_string(provider.provider_id)}\n",
        f"base_url = {toml_string(provider.base_url)}\n",
        'wire_api = "responses"\n',
        "requires_openai_auth = true\n",
    ]
    if query_params:
        block.append(f"query_params = {toml_inline_map(query_params)}\n")
    if headers:
        block.append(f"http_headers = {toml_inline_map(headers)}\n")
    if env_headers:
        block.append(f"env_http_headers = {toml_inline_map(env_headers)}\n")
    return block


def render_cc_switch_config(
    provider: Preset,
    extra_query_params: list[tuple[str, str]],
) -> str:
    lines = [
        f"model_provider = {toml_string(provider.provider_id)}",
        f"model = {toml_string(provider.model)}",
        'model_reasoning_effort = "high"',
        "disable_response_storage = true",
        "",
        f"[model_providers.{provider.provider_id}]",
        f"name = {toml_string(provider.provider_id)}",
        f"base_url = {toml_string(provider.base_url)}",
        'wire_api = "responses"',
        "requires_openai_auth = true",
    ]
    query_params = list(provider.query_params) + extra_query_params
    if query_params:
        lines.append(f"query_params = {toml_inline_map(query_params)}")
    return "\n".join(lines) + "\n"


def update_config(
    config_path: Path,
    provider: Preset,
    headers: list[tuple[str, str]],
    env_headers: list[tuple[str, str]],
    extra_query_params: list[tuple[str, str]],
    *,
    dry_run: bool,
    replace_config: bool = False,
) -> tuple[str, Path | None]:
    config_path.parent.mkdir(parents=True, exist_ok=True)
    if replace_config:
        rendered = render_cc_switch_config(provider, extra_query_params)
    else:
        original = config_path.read_text(encoding="utf-8") if config_path.exists() else ""
        lines = original.splitlines(keepends=True)

        lines = remove_table_family(lines, f"model_providers.{provider.provider_id}")
        root, rest = split_root(lines)
        root = set_root_key(root, "model_provider", provider.provider_id)
        root = set_root_key(root, "model", provider.model)
        root = set_root_key(root, "model_reasoning_effort", "high")
        rendered_lines = root + rest + render_provider_block(
            provider, headers, env_headers, extra_query_params
        )
        rendered = "".join(rendered_lines)

    if tomllib is not None:
        tomllib.loads(rendered)

    backup_path: Path | None = None
    if dry_run:
        return rendered, backup_path

    if config_path.exists():
        stamp = time.strftime("%Y%m%d-%H%M%S")
        backup_path = config_path.with_suffix(f".toml.bak.{stamp}")
        shutil.copy2(config_path, backup_path)

    config_path.write_text(rendered, encoding="utf-8")
    if not is_windows():
        config_path.chmod(0o600)
    return rendered, backup_path


def backup_file(path: Path) -> Path | None:
    if not path.exists():
        return None
    stamp = time.strftime("%Y%m%d-%H%M%S")
    backup_path = path.with_suffix(f"{path.suffix}.bak.{stamp}")
    shutil.copy2(path, backup_path)
    return backup_path


def write_auth_json(auth_path: Path, api_key: str) -> Path | None:
    auth_path.parent.mkdir(parents=True, exist_ok=True)
    backup_path = backup_file(auth_path)
    auth = {CODEX_AUTH_KEY: api_key}
    auth_path.write_text(json.dumps(auth, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    if not is_windows():
        auth_path.chmod(0o600)
    return backup_path


def restore_codex_defaults(codex_home: Path) -> tuple[Path, Path, Path | None, Path | None]:
    config_path = codex_home / "config.toml"
    auth_path = codex_home / "auth.json"
    codex_home.mkdir(parents=True, exist_ok=True)

    config_backup_path = backup_file(config_path)
    auth_backup_path = backup_file(auth_path)

    config_path.write_text("", encoding="utf-8")
    auth_path.write_text("{}\n", encoding="utf-8")
    if not is_windows():
        config_path.chmod(0o600)
        auth_path.chmod(0o600)

    return config_path, auth_path, config_backup_path, auth_backup_path


def read_current_model(config_path: Path) -> str:
    if not config_path.exists():
        return GUI_MODEL_FALLBACK
    for line in config_path.read_text(encoding="utf-8").splitlines():
        if line.lstrip().startswith("#"):
            continue
        match = ROOT_MODEL_RE.match(line)
        if match:
            return match.group(1)
    return GUI_MODEL_FALLBACK


def configure_gui_provider(
    *,
    api_key: str,
    base_url: str,
    codex_home: Path,
    persist_user_env: bool,
) -> tuple[Preset, Path, Path | None, Path]:
    config_path = codex_home / "config.toml"
    auth_path = codex_home / "auth.json"
    model = read_current_model(config_path)
    provider = Preset(
        GUI_PROVIDER_ID,
        GUI_PROVIDER_NAME,
        base_url,
        CODEX_AUTH_KEY,
        model,
    )

    auth_backup_path = None
    old_auth_bytes = auth_path.read_bytes() if auth_path.exists() else None
    try:
        auth_backup_path = write_auth_json(auth_path, api_key)
        _, backup_path = update_config(
            config_path,
            provider,
            headers=[],
            env_headers=[],
            extra_query_params=[],
            dry_run=False,
            replace_config=True,
        )
    except Exception:
        if old_auth_bytes is not None:
            auth_path.write_bytes(old_auth_bytes)
        elif auth_path.exists():
            auth_path.unlink()
        raise

    if auth_backup_path and not backup_path:
        backup_path = auth_backup_path

    return provider, config_path, backup_path, auth_path


def configure_merged_provider(
    *,
    provider: Preset,
    api_key: str | None,
    codex_home: Path,
    headers: list[tuple[str, str]],
    env_headers: list[tuple[str, str]],
    queries: list[tuple[str, str]],
    dry_run: bool,
) -> tuple[str, Path | None, Path | None]:
    config_path = codex_home / "config.toml"
    auth_path = codex_home / "auth.json"
    rendered = render_cc_switch_config(provider, queries)
    if dry_run:
        return rendered, None, None

    auth_backup_path = None
    old_auth_bytes = auth_path.read_bytes() if auth_path.exists() else None
    try:
        if api_key:
            auth_backup_path = write_auth_json(auth_path, api_key)
        _, config_backup_path = update_config(
            config_path,
            provider,
            headers=headers,
            env_headers=env_headers,
            extra_query_params=queries,
            dry_run=False,
            replace_config=True,
        )
    except Exception:
        if old_auth_bytes is not None:
            auth_path.write_bytes(old_auth_bytes)
        elif auth_path.exists():
            auth_path.unlink()
        raise

    return rendered, config_backup_path, auth_backup_path


def update_env_file(env_file: Path, env_key: str, api_key: str) -> None:
    env_file.parent.mkdir(parents=True, exist_ok=True)
    lines = env_file.read_text(encoding="utf-8").splitlines() if env_file.exists() else []
    if env_file.suffix.lower() == ".ps1":
        pattern = re.compile(rf"^\s*\$env:{re.escape(env_key)}\s*=")
        export_line = f"$env:{env_key} = {powershell_single_quote(api_key)}"
    else:
        pattern = re.compile(rf"^\s*export\s+{re.escape(env_key)}=")
        export_line = f"export {env_key}={shell_single_quote(api_key)}"
    replaced = False
    next_lines: list[str] = []
    for line in lines:
        if pattern.match(line):
            next_lines.append(export_line)
            replaced = True
        else:
            next_lines.append(line)
    if not replaced:
        if next_lines and next_lines[-1].strip():
            next_lines.append("")
        next_lines.append(export_line)
    env_file.write_text("\n".join(next_lines) + "\n", encoding="utf-8")
    if not is_windows():
        env_file.chmod(0o600)


def ensure_shell_profile_sources(env_file: Path, shell_profile: Path) -> None:
    shell_profile.parent.mkdir(parents=True, exist_ok=True)
    if shell_profile.suffix.lower() == ".ps1":
        source_line = f'if (Test-Path {powershell_single_quote(str(env_file))}) {{ . {powershell_single_quote(str(env_file))} }}'
    else:
        source_line = f'[ -f "{env_file}" ] && . "{env_file}"'
    if shell_profile.exists():
        content = shell_profile.read_text(encoding="utf-8")
        if source_line in content:
            return
        prefix = "" if content.endswith("\n") else "\n"
    else:
        content = ""
        prefix = ""
    with shell_profile.open("a", encoding="utf-8") as handle:
        handle.write(prefix)
        handle.write("\n# Codex provider API keys\n")
        handle.write(source_line + "\n")


def set_launchctl_env(env_key: str, api_key: str) -> None:
    if platform.system() != "Darwin":
        return
    subprocess.run(["launchctl", "setenv", env_key, api_key], check=False)


def set_windows_user_env(env_key: str, api_key: str) -> None:
    if not is_windows():
        return
    import ctypes
    import winreg

    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment", 0, winreg.KEY_SET_VALUE) as key:
        winreg.SetValueEx(key, env_key, 0, winreg.REG_SZ, api_key)
    os.environ[env_key] = api_key

    hwnd_broadcast = 0xFFFF
    wm_settingchange = 0x001A
    smto_abortifhung = 0x0002
    result = ctypes.c_ulong()
    ctypes.windll.user32.SendMessageTimeoutW(
        hwnd_broadcast,
        wm_settingchange,
        0,
        "Environment",
        smto_abortifhung,
        5000,
        ctypes.byref(result),
    )


def run_gui() -> int:
    try:
        import tkinter as tk
        from tkinter import messagebox
    except Exception as exc:
        print(f"GUI is unavailable: {exc}", file=sys.stderr)
        print("Run with --help for command-line usage.", file=sys.stderr)
        return 1

    window = tk.Tk()
    window.title("OceanWay AI Codex 配置")
    window.resizable(False, False)
    window.configure(bg="#f7f7f8")

    active_field = "api_key"
    api_key_value = ""
    base_url_value = GUI_BASE_URL

    frame = tk.Frame(window, bg="#f7f7f8", padx=32, pady=26)
    frame.pack(fill="both", expand=True)
    frame.columnconfigure(1, weight=1)

    def display_value(value: str, *, hidden: bool = False) -> str:
        if hidden:
            return "API Key: " + ("*" * min(len(value), 40) if value else "点击后输入 API Key")
        if len(value) > 46:
            value = value[:43] + "..."
        return "Base URL: " + (value or "点击后输入 Base URL")

    title_button = tk.Button(
        frame,
        text="OceanWay AI Codex 配置",
        relief="flat",
        borderwidth=0,
        bg="#f7f7f8",
        fg="#111827",
        activebackground="#f7f7f8",
        font=("Arial", 20, "bold"),
    )
    title_button.grid(row=0, column=0, columnspan=4, sticky="w", pady=(0, 22))

    hint_button = tk.Button(
        frame,
        text="点击输入框后直接键盘输入，API Key 会隐藏显示。",
        relief="flat",
        borderwidth=0,
        bg="#f7f7f8",
        fg="#374151",
        activebackground="#f7f7f8",
        font=("Arial", 12),
    )
    hint_button.grid(row=1, column=0, columnspan=4, sticky="w", pady=(0, 12))

    field_options = {
        "bg": "#ffffff",
        "fg": "#111827",
        "activebackground": "#ffffff",
        "activeforeground": "#111827",
        "anchor": "w",
        "relief": "sunken",
        "borderwidth": 2,
        "font": ("Arial", 14),
    }
    api_key_field = tk.Button(frame, text="", width=48, **field_options)
    api_key_field.grid(row=2, column=0, columnspan=4, sticky="ew", pady=(0, 10))
    base_url_field = tk.Button(frame, text="", width=48, **field_options)
    base_url_field.grid(row=3, column=0, columnspan=4, sticky="ew", pady=(0, 12))

    status_button = tk.Button(
        frame,
        text="",
        relief="flat",
        borderwidth=0,
        bg="#f7f7f8",
        fg="#6b7280",
        activebackground="#f7f7f8",
        font=("Arial", 12),
        anchor="w",
    )
    status_button.grid(row=4, column=0, columnspan=4, sticky="ew", pady=(0, 16))

    button_options = {"bg": "#ffffff", "fg": "#111827", "activebackground": "#eeeeee", "font": ("Arial", 13)}
    restore_button = tk.Button(frame, text="恢复默认值", width=12, **button_options)
    restore_button.grid(row=5, column=0, sticky="w")
    spacer = tk.Frame(frame, bg="#f7f7f8")
    spacer.grid(row=5, column=1, sticky="ew")
    exit_button = tk.Button(frame, text="退出", width=8, command=window.destroy, **button_options)
    exit_button.grid(row=5, column=2, padx=(0, 10), sticky="e")
    configure_button = tk.Button(
        frame,
        text="一键配置",
        width=10,
        bg="#111827",
        fg="#ffffff",
        activebackground="#1f2937",
        activeforeground="#ffffff",
        font=("Arial", 13, "bold"),
    )
    configure_button.grid(row=5, column=3, sticky="e")

    def update_fields() -> None:
        cursor = "  |" if active_field == "api_key" else ""
        api_key_field.configure(text=display_value(api_key_value, hidden=True) + cursor)
        cursor = "  |" if active_field == "base_url" else ""
        base_url_field.configure(text=display_value(base_url_value) + cursor)

    def set_active(field: str) -> None:
        nonlocal active_field
        active_field = field
        update_fields()
        window.focus_force()

    def paste_clipboard() -> None:
        nonlocal api_key_value, base_url_value
        try:
            value = window.clipboard_get()
        except Exception:
            return
        if active_field == "api_key":
            api_key_value += value
        else:
            base_url_value += value
        update_fields()

    def clear_active_field() -> None:
        nonlocal api_key_value, base_url_value
        if active_field == "api_key":
            api_key_value = ""
        else:
            base_url_value = ""
        update_fields()

    def handle_key(event: tk.Event) -> str | None:
        nonlocal api_key_value, base_url_value
        key = event.keysym
        state = getattr(event, "state", 0)
        command_or_control = bool(state & 0x0004) or bool(state & 0x0008)
        if command_or_control and key.lower() == "v":
            paste_clipboard()
            return "break"
        if command_or_control and key.lower() == "a":
            clear_active_field()
            return "break"
        if key == "Tab":
            set_active("base_url" if active_field == "api_key" else "api_key")
            return "break"
        if key in {"BackSpace", "Delete"}:
            if active_field == "api_key":
                api_key_value = api_key_value[:-1]
            else:
                base_url_value = base_url_value[:-1]
            update_fields()
            return "break"
        if key in {"Return", "KP_Enter"}:
            configure()
            return "break"
        if key == "Escape":
            window.destroy()
            return "break"
        char = getattr(event, "char", "")
        if char and char >= " " and not command_or_control:
            if active_field == "api_key":
                api_key_value += char
            else:
                base_url_value += char
            update_fields()
            return "break"
        return None

    api_key_field.configure(command=lambda: set_active("api_key"))
    base_url_field.configure(command=lambda: set_active("base_url"))
    window.bind("<Key>", handle_key)
    window.bind("<Button-1>", lambda _event: window.focus_force(), add="+")

    update_fields()

    def set_busy(message: str) -> None:
        status_button.configure(text=message)
        configure_button.configure(state="disabled")
        restore_button.configure(state="disabled")
        api_key_field.configure(state="disabled")
        base_url_field.configure(state="disabled")
        window.update_idletasks()

    def set_ready(message: str = "") -> None:
        status_button.configure(text=message)
        configure_button.configure(state="normal")
        restore_button.configure(state="normal")
        api_key_field.configure(state="normal")
        base_url_field.configure(state="normal")
        window.update_idletasks()

    def configure() -> None:
        api_key = api_key_value.strip()
        if not api_key:
            messagebox.showerror("缺少 API Key", "请先填写 API Key。", parent=window)
            set_active("api_key")
            return

        base_url = base_url_value.strip()
        if not (base_url.startswith("http://") or base_url.startswith("https://")):
            messagebox.showerror("Base URL 无效", "Base URL 需要以 http:// 或 https:// 开头。", parent=window)
            set_active("base_url")
            return

        try:
            set_busy("正在写入 Codex 配置...")
            codex_home = Path(os.environ.get("CODEX_HOME", str(Path.home() / ".codex"))).expanduser()
            provider, config_path, backup_path, auth_path = configure_gui_provider(
                api_key=api_key,
                base_url=base_url,
                codex_home=codex_home,
                persist_user_env=False,
            )
        except Exception as exc:
            set_ready()
            messagebox.showerror("配置失败", str(exc), parent=window)
            return

        set_ready("配置完成。请重启 Codex。")
        backup_text = f"\n备份文件：{backup_path}" if backup_path else ""
        messagebox.showinfo(
            "配置完成",
            "Codex 第三方提供商已配置完成。\n\n"
            f"Provider：{provider.provider_id}\n"
            f"Model：{provider.model}\n"
            f"配置文件：{config_path}\n"
            f"密钥文件：{auth_path}"
            f"{backup_text}"
            "\n\n"
            "请重启 Codex 后使用。",
            parent=window,
        )

    def restore_defaults() -> None:
        answer = messagebox.askyesno(
            "恢复默认值",
            "这会备份并清空 Codex 配置，恢复为无第三方提供商的默认状态。\n\n是否继续？",
            parent=window,
        )
        if not answer:
            return

        try:
            set_busy("正在恢复默认值...")
            codex_home = Path(os.environ.get("CODEX_HOME", str(Path.home() / ".codex"))).expanduser()
            config_path, auth_path, config_backup_path, auth_backup_path = restore_codex_defaults(codex_home)
        except Exception as exc:
            set_ready()
            messagebox.showerror("恢复失败", str(exc), parent=window)
            return

        set_ready("已恢复默认值。请重启 Codex。")
        backups = []
        if config_backup_path:
            backups.append(f"配置备份：{config_backup_path}")
        if auth_backup_path:
            backups.append(f"密钥备份：{auth_backup_path}")
        backup_text = "\n".join(backups) if backups else "此前没有可备份的配置文件。"
        messagebox.showinfo(
            "恢复完成",
            "Codex 已恢复为无第三方提供商配置状态。\n\n"
            f"配置文件：{config_path}\n"
            f"密钥文件：{auth_path}\n\n"
            f"{backup_text}\n\n"
            "请重启 Codex 后使用。",
            parent=window,
        )

    configure_button.configure(command=configure)
    restore_button.configure(command=restore_defaults)

    set_active("api_key")
    window.update_idletasks()
    width = 560
    height = 300
    x = (window.winfo_screenwidth() - width) // 2
    y = (window.winfo_screenheight() - height) // 3
    window.geometry(f"{width}x{height}+{x}+{y}")
    window.lift()
    window.focus_force()
    window.attributes("-topmost", True)
    window.after(800, lambda: window.attributes("-topmost", False))
    window.mainloop()
    return 0


def resolve_provider(args: argparse.Namespace) -> Preset:
    preset = PRESETS[args.provider] if args.provider else choose_preset()

    provider_id = args.provider_id or preset.provider_id
    name = args.name or preset.name
    base_url = args.base_url or preset.base_url
    env_key = CODEX_AUTH_KEY
    model = args.model or preset.model

    if args.interactive or not args.provider:
        provider_id = prompt(provider_id, "Provider id")
        name = prompt(name, "Display name")
        base_url = prompt(base_url, "Base URL")
        model = prompt(model, "Model name")

    if not VALID_PROVIDER_ID.match(provider_id):
        raise SystemExit("Provider id may only contain letters, numbers, underscores, and hyphens.")
    if provider_id in RESERVED_PROVIDER_IDS:
        raise SystemExit(
            f"Provider id {provider_id!r} is reserved by Codex. Choose another id."
        )
    if not base_url:
        raise SystemExit("Base URL is required.")
    if not model:
        raise SystemExit("Model is required. Pass --model or answer the interactive prompt.")
    return replace(
        preset,
        provider_id=provider_id,
        name=name,
        base_url=base_url,
        env_key=env_key,
        model=model,
    )


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Configure Codex to use a third-party Responses-compatible provider."
    )
    parser.add_argument("--gui", action="store_true", help="Open the graphical setup window.")
    parser.add_argument("--cli", action="store_true", help="Use the command-line interactive flow.")
    parser.add_argument(
        "--provider",
        choices=sorted(PRESETS),
        help="Provider preset. Omit for an interactive menu.",
    )
    parser.add_argument("--provider-id", help="Override provider id written under [model_providers.<id>].")
    parser.add_argument("--name", help="Override provider display name.")
    parser.add_argument("--base-url", help="Override provider API base URL.")
    parser.add_argument("--env-key", help="Deprecated. cc-switch mode always writes OPENAI_API_KEY to auth.json.")
    parser.add_argument("--model", help="Model name to make active in Codex.")
    parser.add_argument("--api-key", help="API key to save in ~/.codex/auth.json as OPENAI_API_KEY.")
    parser.add_argument(
        "--write-env",
        action="store_true",
        help="Prompt for and save the API key to ~/.codex/auth.json when --api-key is not provided.",
    )
    parser.add_argument(
        "--persist-user-env",
        action="store_true",
        help="Deprecated. cc-switch mode does not use user environment variables.",
    )
    parser.add_argument(
        "--source-shell",
        action="store_true",
        help="Deprecated. cc-switch mode does not use shell profile source lines.",
    )
    parser.add_argument(
        "--shell-profile",
        default=None,
        help="Shell profile to update when --source-shell is used.",
    )
    parser.add_argument(
        "--launchctl",
        action="store_true",
        help="Deprecated. cc-switch mode does not use launchctl environment variables.",
    )
    parser.add_argument(
        "--header",
        action="append",
        default=[],
        metavar="HEADER=VALUE",
        help="Static HTTP header to add to the provider config. Repeatable.",
    )
    parser.add_argument(
        "--env-header",
        action="append",
        default=[],
        metavar="HEADER=ENV_VAR",
        help="HTTP header populated from an environment variable. Repeatable.",
    )
    parser.add_argument(
        "--query",
        action="append",
        default=[],
        metavar="KEY=VALUE",
        help="Query parameter to add to the provider config. Repeatable.",
    )
    parser.add_argument(
        "--codex-home",
        default=os.environ.get("CODEX_HOME", str(Path.home() / ".codex")),
        help="Codex home directory. Defaults to CODEX_HOME or ~/.codex.",
    )
    parser.add_argument("--dry-run", action="store_true", help="Print the generated config instead of writing it.")
    parser.add_argument("--list", action="store_true", help="List provider presets and exit.")
    parser.add_argument(
        "--interactive",
        action="store_true",
        help="Prompt for all fields even when --provider is supplied.",
    )
    return parser


def main() -> int:
    if len(sys.argv) == 1:
        return run_gui()

    parser = build_arg_parser()
    args = parser.parse_args()

    if args.gui:
        return run_gui()

    if args.list:
        for key, preset in PRESETS.items():
            model = f", default model: {preset.model}" if preset.model else ""
            print(f"{key:<12} {preset.name} ({preset.base_url or 'prompted'}{model})")
        return 0

    provider = resolve_provider(args)
    headers = parse_pairs(args.header, "--header")
    env_headers = parse_pairs(args.env_header, "--env-header")
    queries = parse_pairs(args.query, "--query")

    codex_home = Path(args.codex_home).expanduser()
    config_path = codex_home / "config.toml"
    auth_path = codex_home / "auth.json"

    api_key = args.api_key
    if args.write_env and not api_key:
        api_key = getpass.getpass(f"Paste {CODEX_AUTH_KEY} (input hidden): ").strip()

    rendered, backup_path, auth_backup_path = configure_merged_provider(
        provider=provider,
        api_key=api_key,
        codex_home=codex_home,
        headers=headers,
        env_headers=env_headers,
        queries=queries,
        dry_run=args.dry_run,
    )

    if args.dry_run:
        print(rendered)
        return 0

    print(f"Updated: {config_path}")
    if backup_path:
        print(f"Config backup: {backup_path}")
    if auth_backup_path:
        print(f"Auth backup:   {auth_backup_path}")
    print(f"Active:  model_provider={provider.provider_id}, model={provider.model}")
    print(f"API key: {CODEX_AUTH_KEY}")
    if api_key:
        print(f"Saved:   {auth_path}")
    else:
        print(f"Next:    write {CODEX_AUTH_KEY} to {auth_path}")
    print("Restart Codex after changing provider configuration.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
