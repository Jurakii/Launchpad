param(
  [Parameter(Mandatory=$true)][ValidateSet('get','set','mute')]
  [string]$Action,
  [string]$Value
)

# No official .NET/PowerShell API exposes the system volume, so this talks
# directly to the Windows Core Audio COM interfaces (IMMDeviceEnumerator /
# IAudioEndpointVolume). The placeholder f()/f2() methods below stand in for
# COM vtable slots (RegisterControlChangeNotify, UnregisterControlChangeNotify,
# EnumAudioEndpoints) that this script doesn't call but must still declare in
# order, or every later method call would hit the wrong vtable slot.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

[Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IAudioEndpointVolume {
    int f(); int f2();
    int GetChannelCount(out uint pnChannelCount);
    int SetMasterVolumeLevel(float fLevelDB, Guid pguidEventContext);
    int SetMasterVolumeLevelScalar(float fLevel, Guid pguidEventContext);
    int GetMasterVolumeLevel(out float pfLevelDB);
    int GetMasterVolumeLevelScalar(out float pfLevel);
    int SetChannelVolumeLevel(uint nChannel, float fLevelDB, Guid pguidEventContext);
    int SetChannelVolumeLevelScalar(uint nChannel, float fLevel, Guid pguidEventContext);
    int GetChannelVolumeLevel(uint nChannel, out float pfLevelDB);
    int GetChannelVolumeLevelScalar(uint nChannel, out float pfLevel);
    int SetMute([MarshalAs(UnmanagedType.Bool)] bool bMute, Guid pguidEventContext);
    int GetMute(out bool pbMute);
    int GetVolumeStepInfo(out uint pnStep, out uint pnStepCount);
    int VolumeStepUp(Guid pguidEventContext);
    int VolumeStepDown(Guid pguidEventContext);
    int QueryHardwareSupport(out uint pdwHardwareSupportMask);
    int GetVolumeRange(out float pflVolumeMindB, out float pflVolumeMaxdB, out float pflVolumeIncrementdB);
}

[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMMDevice {
    int Activate(ref Guid id, int clsCtx, int activationParams, out IAudioEndpointVolume aev);
}

[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMMDeviceEnumerator {
    int f();
    int GetDefaultAudioEndpoint(int dataFlow, int role, out IMMDevice ppDevice);
}

[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")]
public class MMDeviceEnumeratorComObject { }

public class Audio {
    static IAudioEndpointVolume Vol() {
        var enumerator = new MMDeviceEnumeratorComObject() as IMMDeviceEnumerator;
        IMMDevice dev = null;
        Marshal.ThrowExceptionForHR(enumerator.GetDefaultAudioEndpoint(0, 1, out dev));
        IAudioEndpointVolume epv = null;
        var epvid = typeof(IAudioEndpointVolume).GUID;
        Marshal.ThrowExceptionForHR(dev.Activate(ref epvid, 23, 0, out epv));
        return epv;
    }
    public static float GetVolume() {
        float v = -1;
        Marshal.ThrowExceptionForHR(Vol().GetMasterVolumeLevelScalar(out v));
        return v;
    }
    public static void SetVolume(float level) {
        Marshal.ThrowExceptionForHR(Vol().SetMasterVolumeLevelScalar(level, Guid.Empty));
    }
    public static bool GetMute() {
        bool mute;
        Marshal.ThrowExceptionForHR(Vol().GetMute(out mute));
        return mute;
    }
    public static void SetMute(bool mute) {
        Marshal.ThrowExceptionForHR(Vol().SetMute(mute, Guid.Empty));
    }
}
'@

switch ($Action) {
  'get' {
    $vol = [Audio]::GetVolume()
    $muted = [Audio]::GetMute()
    $pct = [Math]::Round($vol * 100)
    Write-Output "{`"volume`":$pct,`"muted`":$($muted.ToString().ToLower())}"
  }
  'set' {
    $level = [double]$Value / 100.0
    if ($level -lt 0) { $level = 0 }
    if ($level -gt 1) { $level = 1 }
    [Audio]::SetVolume($level)
  }
  'mute' {
    [Audio]::SetMute($Value -eq 'true')
  }
}
