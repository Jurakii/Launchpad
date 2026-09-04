param(
  [Parameter(Mandatory=$true)][ValidateSet('listDevices','setDevice','listSessions','setSessionVolume','setSessionMuted')]
  [string]$Action,
  [string]$DeviceId,
  [string]$ProcessId,
  [string]$Value
)

# Device enumeration/friendly-names and the per-app session mixer both use
# documented public Core Audio interfaces. Switching the *default* device
# has no public API at all - every tool that does it (this script included)
# relies on the undocumented IPolicyConfig COM interface, stable since
# Windows 7 but not guaranteed by Microsoft; SetDefaultDevice below fails
# safely (caught, reported as {"ok":false}) rather than crashing if a future
# Windows build changes its shape.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

[StructLayout(LayoutKind.Sequential)]
public struct PROPERTYKEY {
    public Guid fmtid;
    public int pid;
}

[StructLayout(LayoutKind.Explicit)]
public struct PROPVARIANT {
    [FieldOffset(0)] public ushort vt;
    [FieldOffset(8)] public IntPtr pwszVal;
}

[Guid("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IPropertyStore {
    int GetCount(out uint cProps);
    int GetAt(uint iProp, out PROPERTYKEY pkey);
    int GetValue(ref PROPERTYKEY key, out PROPVARIANT pv);
    int SetValue(ref PROPERTYKEY key, ref PROPVARIANT propvar);
    int Commit();
}

[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMMDevice {
    int Activate(ref Guid iid, int clsCtx, int activationParams, out IAudioSessionManager2 aev);
    int OpenPropertyStore(int stgmAccess, out IPropertyStore ppProperties);
    int GetId([MarshalAs(UnmanagedType.LPWStr)] out string ppstrId);
    int GetState(out int pdwState);
}

[Guid("0BD7A1BE-7A1A-44DB-8397-CC5392387B5E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMMDeviceCollection {
    int GetCount(out uint pcDevices);
    int Item(uint nDevice, out IMMDevice ppDevice);
}

[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMMDeviceEnumerator {
    int EnumAudioEndpoints(int dataFlow, int dwStateMask, out IMMDeviceCollection ppDevices);
    int GetDefaultAudioEndpoint(int dataFlow, int role, out IMMDevice ppEndpoint);
}

[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")]
public class MMDeviceEnumeratorComObject { }

[Guid("f8679f50-850a-41cf-9c72-430f290290c8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IPolicyConfig {
    int GetMixFormat(string deviceId, IntPtr ppFormat);
    int GetDeviceFormat(string deviceId, bool bDefault, IntPtr ppFormat);
    int ResetDeviceFormat(string deviceId);
    int SetDeviceFormat(string deviceId, IntPtr endpointFormat, IntPtr mixFormat);
    int GetProcessingPeriod(string deviceId, bool bDefault, IntPtr defaultPeriod, IntPtr minimumPeriod);
    int SetProcessingPeriod(string deviceId, IntPtr period);
    int GetShareMode(string deviceId, IntPtr mode);
    int SetShareMode(string deviceId, IntPtr mode);
    int GetPropertyValue(string deviceId, ref PROPERTYKEY key, IntPtr pv);
    int SetPropertyValue(string deviceId, ref PROPERTYKEY key, IntPtr pv);
    int SetDefaultEndpoint(string deviceId, int role);
    int SetEndpointVisibility(string deviceId, bool visible);
}

[ComImport, Guid("870af99c-171d-4f9e-af0d-e63df40c2bc9")]
public class PolicyConfigClient { }

[Guid("77AA99A0-1BD6-484F-8BC7-2C654C9A9B6F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IAudioSessionManager2 {
    int f1();
    int f2();
    int GetSessionEnumerator(out IAudioSessionEnumerator SessionEnum);
}

[Guid("E2F5BB11-0570-40CA-ACDD-3AA01277DEE8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IAudioSessionEnumerator {
    int GetCount(out int SessionCount);
    int GetSession(int SessionIndex, out IAudioSessionControl2 Session);
}

[Guid("bfb7ff88-7239-4fc9-8fa2-07c950be9c6d"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IAudioSessionControl2 {
    int GetState(out int pRetVal);
    int GetDisplayName([MarshalAs(UnmanagedType.LPWStr)] out string pRetVal);
    int SetDisplayName(string Value, ref Guid EventContext);
    int GetIconPath([MarshalAs(UnmanagedType.LPWStr)] out string pRetVal);
    int SetIconPath(string Value, ref Guid EventContext);
    int GetGroupingParam(out Guid pRetVal);
    int SetGroupingParam(ref Guid Override, ref Guid EventContext);
    int RegisterAudioSessionNotification(IntPtr NewNotifications);
    int UnregisterAudioSessionNotification(IntPtr NewNotifications);
    int GetSessionIdentifier([MarshalAs(UnmanagedType.LPWStr)] out string pRetVal);
    int GetSessionInstanceIdentifier([MarshalAs(UnmanagedType.LPWStr)] out string pRetVal);
    int GetProcessId(out uint pRetVal);
    int IsSystemSoundsSession();
    int SetDuckingPreference(bool optOut);
}

[Guid("87CE5498-68D6-44E5-9215-6DA47EF883D8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface ISimpleAudioVolume {
    int SetMasterVolume(float fLevel, ref Guid EventContext);
    int GetMasterVolume(out float pfLevel);
    int SetMute([MarshalAs(UnmanagedType.Bool)] bool bMute, ref Guid EventContext);
    int GetMute(out bool pbMute);
}

public class AudioDevices {
    [DllImport("ole32.dll")]
    static extern int PropVariantClear(ref PROPVARIANT pvar);

    static IMMDeviceEnumerator Enumerator() {
        return new MMDeviceEnumeratorComObject() as IMMDeviceEnumerator;
    }

    static string JsonEscape(string s) {
        if (s == null) return "";
        return s.Replace("\\", "\\\\").Replace("\"", "\\\"");
    }

    static string GetFriendlyName(IMMDevice device) {
        IPropertyStore store;
        Marshal.ThrowExceptionForHR(device.OpenPropertyStore(0, out store));
        var key = new PROPERTYKEY { fmtid = new Guid("a45c254e-df1c-4efd-8020-67d146a850e0"), pid = 14 };
        PROPVARIANT pv;
        Marshal.ThrowExceptionForHR(store.GetValue(ref key, out pv));
        string name = pv.pwszVal != IntPtr.Zero ? Marshal.PtrToStringUni(pv.pwszVal) : "Unknown device";
        try { PropVariantClear(ref pv); } catch { }
        return name;
    }

    public static string ListDevicesJson() {
        var enumerator = Enumerator();
        IMMDeviceCollection collection;
        Marshal.ThrowExceptionForHR(enumerator.EnumAudioEndpoints(0, 1, out collection));
        uint count;
        Marshal.ThrowExceptionForHR(collection.GetCount(out count));

        string defaultId = null;
        try {
            IMMDevice defaultDevice;
            Marshal.ThrowExceptionForHR(enumerator.GetDefaultAudioEndpoint(0, 1, out defaultDevice));
            defaultDevice.GetId(out defaultId);
        } catch { }

        var sb = new StringBuilder();
        sb.Append("[");
        for (uint i = 0; i < count; i++) {
            IMMDevice dev;
            Marshal.ThrowExceptionForHR(collection.Item(i, out dev));
            string id;
            dev.GetId(out id);
            string name = GetFriendlyName(dev);
            bool isDefault = defaultId != null && id == defaultId;
            if (i > 0) sb.Append(",");
            sb.Append("{\"id\":\"" + JsonEscape(id) + "\",\"name\":\"" + JsonEscape(name) + "\",\"isDefault\":" + (isDefault ? "true" : "false") + "}");
        }
        sb.Append("]");
        return sb.ToString();
    }

    public static void SetDefaultDevice(string deviceId) {
        var policyConfig = new PolicyConfigClient() as IPolicyConfig;
        Marshal.ThrowExceptionForHR(policyConfig.SetDefaultEndpoint(deviceId, 0));
        Marshal.ThrowExceptionForHR(policyConfig.SetDefaultEndpoint(deviceId, 1));
        Marshal.ThrowExceptionForHR(policyConfig.SetDefaultEndpoint(deviceId, 2));
    }

    static IAudioSessionManager2 GetSessionManager() {
        var enumerator = Enumerator();
        IMMDevice device;
        Marshal.ThrowExceptionForHR(enumerator.GetDefaultAudioEndpoint(0, 1, out device));
        var iid = typeof(IAudioSessionManager2).GUID;
        IAudioSessionManager2 mgr;
        Marshal.ThrowExceptionForHR(device.Activate(ref iid, 23, 0, out mgr));
        return mgr;
    }

    public static string ListSessionsJson() {
        var mgr = GetSessionManager();
        IAudioSessionEnumerator sessionEnum;
        Marshal.ThrowExceptionForHR(mgr.GetSessionEnumerator(out sessionEnum));
        int count;
        Marshal.ThrowExceptionForHR(sessionEnum.GetCount(out count));

        var sb = new StringBuilder();
        sb.Append("[");
        bool first = true;
        for (int i = 0; i < count; i++) {
            IAudioSessionControl2 session;
            Marshal.ThrowExceptionForHR(sessionEnum.GetSession(i, out session));

            int state;
            session.GetState(out state);
            if (state == 2) continue; // AudioSessionStateExpired

            uint pid;
            session.GetProcessId(out pid);
            // The system-sounds session isn't owned by any process (reports pid 0) -
            // IsSystemSoundsSession() exists to detect it too, but returned unreliable
            // values in testing here, so pid 0 is used as the (simpler, more robust) signal.
            if (pid == 0) continue;

            string name;
            try { name = System.Diagnostics.Process.GetProcessById((int)pid).ProcessName; }
            catch { name = "pid " + pid; }

            var vol = (ISimpleAudioVolume)session;
            float level = 0;
            bool muted = false;
            try { vol.GetMasterVolume(out level); } catch { }
            try { vol.GetMute(out muted); } catch { }

            if (!first) sb.Append(",");
            first = false;
            sb.Append("{\"pid\":" + pid + ",\"name\":\"" + JsonEscape(name) + "\",\"volume\":" + (int)Math.Round(level * 100) + ",\"muted\":" + (muted ? "true" : "false") + "}");
        }
        sb.Append("]");
        return sb.ToString();
    }

    static void ForEachSessionWithPid(uint pid, Action<ISimpleAudioVolume> action) {
        var mgr = GetSessionManager();
        IAudioSessionEnumerator sessionEnum;
        Marshal.ThrowExceptionForHR(mgr.GetSessionEnumerator(out sessionEnum));
        int count;
        Marshal.ThrowExceptionForHR(sessionEnum.GetCount(out count));
        for (int i = 0; i < count; i++) {
            IAudioSessionControl2 session;
            Marshal.ThrowExceptionForHR(sessionEnum.GetSession(i, out session));
            uint sessPid;
            session.GetProcessId(out sessPid);
            if (sessPid == pid) action((ISimpleAudioVolume)session);
        }
    }

    public static void SetSessionVolume(uint pid, float level) {
        var ctx = Guid.Empty;
        ForEachSessionWithPid(pid, vol => Marshal.ThrowExceptionForHR(vol.SetMasterVolume(level, ref ctx)));
    }

    public static void SetSessionMuted(uint pid, bool muted) {
        var ctx = Guid.Empty;
        ForEachSessionWithPid(pid, vol => Marshal.ThrowExceptionForHR(vol.SetMute(muted, ref ctx)));
    }
}
'@

function Write-Result($fn) {
  try {
    & $fn
  } catch {
    $msg = $_.Exception.Message -replace '"','\"'
    Write-Output "{`"ok`":false,`"error`":`"$msg`"}"
  }
}

switch ($Action) {
  'listDevices' { Write-Result { Write-Output ([AudioDevices]::ListDevicesJson()) } }
  'setDevice' { Write-Result { [AudioDevices]::SetDefaultDevice($DeviceId); Write-Output '{"ok":true}' } }
  'listSessions' { Write-Result { Write-Output ([AudioDevices]::ListSessionsJson()) } }
  'setSessionVolume' { Write-Result { [AudioDevices]::SetSessionVolume([uint32]$ProcessId, [float]([double]$Value / 100.0)); Write-Output '{"ok":true}' } }
  'setSessionMuted' { Write-Result { [AudioDevices]::SetSessionMuted([uint32]$ProcessId, ($Value -eq 'true')); Write-Output '{"ok":true}' } }
}
