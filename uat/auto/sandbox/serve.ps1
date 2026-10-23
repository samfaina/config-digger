# Runs inside Windows Sandbox for step 12.2: the updater's endpoint. Serves
# C:\uat\update on http://127.0.0.1:8765/ and logs each request to
# C:\uat\results\serve.log.
$listener = New-Object System.Net.HttpListener
$listener.Prefixes.Add("http://127.0.0.1:8765/")
$listener.Start()
while ($listener.IsListening) {
    $context = $listener.GetContext()
    $name = [IO.Path]::GetFileName($context.Request.Url.AbsolutePath)
    $path = Join-Path C:\uat\update $name
    Add-Content C:\uat\results\serve.log "$((Get-Date).ToString('o')) $($context.Request.HttpMethod) $($context.Request.Url.AbsolutePath) ua=$($context.Request.UserAgent)"
    if ($name -and (Test-Path $path -PathType Leaf)) {
        $bytes = [IO.File]::ReadAllBytes($path)
        $context.Response.ContentType = if ($name.EndsWith(".json")) { "application/json" } else { "application/octet-stream" }
        $context.Response.ContentLength64 = $bytes.Length
        $context.Response.OutputStream.Write($bytes, 0, $bytes.Length)
    } else {
        $context.Response.StatusCode = 404
    }
    $context.Response.Close()
}
