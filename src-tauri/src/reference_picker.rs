use std::process::Command;

#[tauri::command]
pub(super) async fn pick_reference_images() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(pick_files).await
        .map_err(|_| "参考图选择任务异常。".to_string())?
}

fn pick_files() -> Result<Vec<String>, String> {
    #[cfg(target_os = "windows")]
    {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use std::os::windows::process::CommandExt;
        let script = r#"
Add-Type -AssemblyName System.Windows.Forms
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
$dialog=New-Object System.Windows.Forms.OpenFileDialog
$dialog.Title='选择参考图'
$dialog.Filter='Images (*.png;*.jpg;*.jpeg;*.webp)|*.png;*.jpg;*.jpeg;*.webp'
$dialog.Multiselect=$true
$dialog.CheckFileExists=$true
if($dialog.ShowDialog() -eq 'OK'){ConvertTo-Json -InputObject @($dialog.FileNames) -Compress}else{'[]'}
$dialog.Dispose()
"#;
        let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-STA", "-EncodedCommand", &STANDARD.encode(bytes)])
            .creation_flags(0x08000000).output()
            .map_err(|_| "无法打开系统图片选择器。".to_string())?;
        if !output.status.success() { return Err("系统图片选择器失败。".into()); }
        return serde_json::from_slice(&output.stdout).map_err(|_| "无法读取选择的文件路径。".into());
    }
    #[cfg(target_os = "macos")]
    {
        let script = r#"
try
    set chosen to choose file with prompt "选择参考图" of type {"public.png", "public.jpeg", "org.webmproject.webp"} with multiple selections allowed
    set resultText to ""
    repeat with f in chosen
        set resultText to resultText & POSIX path of f & linefeed
    end repeat
    return resultText
on error number -128
    return ""
end try
"#;
        let output = Command::new("osascript").args(["-e", script]).output()
            .map_err(|_| "无法打开系统图片选择器。".to_string())?;
        if !output.status.success() { return Err("系统图片选择器失败。".into()); }
        return Ok(String::from_utf8_lossy(&output.stdout).lines()
            .filter(|s| !s.is_empty()).map(str::to_string).collect());
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    Err("当前平台尚未实现参考图选择器。".into())
}
