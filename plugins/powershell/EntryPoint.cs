using System.Management.Automation;
using System.Management.Automation.Runspaces;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace Sst.PowerShell.Bridge;

public sealed class Invocation
{
    [JsonPropertyName("kind")] public string Kind { get; set; } = "script";
    [JsonPropertyName("script")] public string Script { get; set; } = "";
    [JsonPropertyName("path")] public string Path { get; set; } = "";
    [JsonPropertyName("command")] public string Command { get; set; } = "";
    [JsonPropertyName("arguments")] public string[] Arguments { get; set; } = [];
    [JsonPropertyName("cwd")] public string Cwd { get; set; } = "";
    [JsonPropertyName("stdin")] public string? Stdin { get; set; }
}

public sealed class Reply
{
    [JsonPropertyName("status")] public int Status { get; set; }
    [JsonPropertyName("stdout")] public string Stdout { get; set; } = "";
    [JsonPropertyName("stderr")] public string Stderr { get; set; } = "";
}

/// <summary>
/// Actual PowerShell 7 runspace hosted in the SST process.
/// There is no pwsh.exe and no command-line or temporary file transport.
/// EntryPoint.Execute accepts UTF-8 JSON and returns CoTaskMem-allocated UTF-8 JSON.
/// SST releases the returned pointer with CoTaskMemFree.
/// </summary>
public static class EntryPoint
{
    private static readonly object Sync = new();
    private static Runspace? _runspace;
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
        MaxDepth = 32
    };

    private static string Quote(string value) => "'" + value.Replace("'", "''") + "'";

    // The assembly is published self-contained as an app only to activate the
    // bundled .NET runtime through hostfxr. SST never executes this entrypoint.
    public static int Main(string[] _)
    {
        Console.Error.WriteLine("Este motor debe cargarse desde SST mediante hostfxr.");
        return 2;
    }

    [UnmanagedCallersOnly]
    public static unsafe int Execute(nint requestUtf8, nint* responseUtf8)
    {
        if (responseUtf8 == null || requestUtf8 == 0) return 87;
        *responseUtf8 = 0;
        Reply answer;
        try
        {
            string raw = Marshal.PtrToStringUTF8(requestUtf8)
                ?? throw new ArgumentException("Solicitud vacía");
            Invocation request = JsonSerializer.Deserialize<Invocation>(raw, JsonOptions)
                ?? throw new ArgumentException("Solicitud no válida");
            lock (Sync)
            {
                answer = Run(request);
            }
        }
        catch (Exception error)
        {
            // Never include untrusted request contents in error messages.
            answer = new Reply
            {
                Status = 2,
                Stderr = "SST PowerShell: " + error.GetType().Name + ": " + error.Message + "\n"
            };
        }

        string serialized = JsonSerializer.Serialize(answer, JsonOptions);
        *responseUtf8 = Marshal.StringToCoTaskMemUTF8(serialized);
        return *responseUtf8 == 0 ? 12 : 0;
    }

    private static Reply Run(Invocation request)
    {
         _runspace ??= RunspaceFactory.CreateRunspace(InitialSessionState.CreateDefault2());
        if (_runspace.RunspaceStateInfo.State != RunspaceState.Opened)
            _runspace.Open();

        if (!string.IsNullOrWhiteSpace(request.Cwd))
        {
            // Session-wide current location, never mutate the native process CWD.
            _runspace.SessionStateProxy.Path.SetLocation(request.Cwd);
        }

        using System.Management.Automation.PowerShell ps = System.Management.Automation.PowerShell.Create();
        ps.Runspace = _runspace;
        switch (request.Kind)
        {
            case "probe":
                ps.AddScript("$PSVersionTable.PSVersion.ToString()");
                break;
            case "script":
                ps.AddScript(request.Script ?? "", useLocalScope: false);
                break;
            case "file":
                if (string.IsNullOrWhiteSpace(request.Path) ||
                    !System.IO.File.Exists(request.Path) ||
                    !request.Path.EndsWith(".ps1", StringComparison.OrdinalIgnoreCase))
                    return new Reply { Status = 2, Stderr = "PowerShell: archivo .ps1 no encontrado\n" };
                // PowerShell syntax is parsed by its actual engine. Quote data
                // tokens separately; do not allow metacharacters in option names.
                string args = string.Join(" ", request.Arguments.Select(value =>
                    System.Text.RegularExpressions.Regex.IsMatch(value, @"^-[A-Za-z][A-Za-z0-9-]*$")
                        ? value : Quote(value)));
                ps.AddScript("& " + Quote(System.IO.Path.GetFullPath(request.Path)) + " " + args,
                    useLocalScope: false);
                break;
            case "command":
                if (string.IsNullOrWhiteSpace(request.Command))
                    return new Reply { Status = 2, Stderr = "PowerShell: cmdlet ausente\n" };
                ps.AddCommand(request.Command);
                for (int i = 0; i < request.Arguments.Length; i++)
                {
                    var value = request.Arguments[i];
                    if (value.StartsWith("-") && value.Length > 1 &&
                        System.Text.RegularExpressions.Regex.IsMatch(value, @"^-[A-Za-z][A-Za-z0-9-]*$"))
                    {
                        string name = value[1..];
                        if (i + 1 < request.Arguments.Length && !request.Arguments[i + 1].StartsWith("-"))
                            ps.AddParameter(name, request.Arguments[++i]);
                        else
                            ps.AddParameter(name);
                    }
                    else ps.AddArgument(value);
                }
                break;
            default:
                return new Reply { Status = 2, Stderr = "PowerShell: operación desconocida\n" };
        }

        // Formatting stays inside the PowerShell engine, so structured objects,
        // pipelines and formatting cmdlets work as they do in real PowerShell.
        ps.AddCommand("Out-String").AddParameter("Width", 180);
        var output = request.Stdin is null
            ? ps.Invoke()
            : ps.Invoke(new[] { request.Stdin });
        var stdout = new StringBuilder();
        foreach (PSObject item in output)
            if (item?.BaseObject is not null) stdout.Append(item.BaseObject.ToString());
        foreach (InformationRecord message in ps.Streams.Information)
            if (message.MessageData is not null) stdout.AppendLine(message.MessageData.ToString());
        var stderr = new StringBuilder();
        foreach (ErrorRecord error in ps.Streams.Error)
            stderr.AppendLine(error.ToString());
        return new Reply
        {
            Status = ps.HadErrors ? 1 : 0,
            Stdout = stdout.ToString(),
            Stderr = stderr.ToString()
        };
    }
}
