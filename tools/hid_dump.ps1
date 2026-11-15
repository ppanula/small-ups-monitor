#requires -Version 5
# Read-only probe of APC USB HID UPS (VID 051D).
# Enumerates HID collections, prints HID value caps, then reads feature/input reports.
# No installs, no driver replacement, no writes to the UPS.

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;

public static class HidProbe
{
    public const int DIGCF_PRESENT = 0x02;
    public const int DIGCF_DEVICEINTERFACE = 0x10;
    public const uint GENERIC_READ = 0x80000000;
    public const uint GENERIC_WRITE = 0x40000000;
    public const uint FILE_SHARE_READ = 0x1;
    public const uint FILE_SHARE_WRITE = 0x2;
    public const uint OPEN_EXISTING = 3;
    public const int HidP_Input = 0;
    public const int HidP_Output = 1;
    public const int HidP_Feature = 2;

    [StructLayout(LayoutKind.Sequential)]
    public struct SP_DEVICE_INTERFACE_DATA
    {
        public int cbSize;
        public Guid InterfaceClassGuid;
        public int Flags;
        public IntPtr Reserved;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct HIDD_ATTRIBUTES
    {
        public int Size;
        public ushort VendorID;
        public ushort ProductID;
        public ushort VersionNumber;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct HIDP_CAPS
    {
        public ushort Usage;
        public ushort UsagePage;
        public ushort InputReportByteLength;
        public ushort OutputReportByteLength;
        public ushort FeatureReportByteLength;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 17)]
        public ushort[] Reserved;
        public ushort NumberLinkCollectionNodes;
        public ushort NumberInputButtonCaps;
        public ushort NumberInputValueCaps;
        public ushort NumberInputDataIndices;
        public ushort NumberOutputButtonCaps;
        public ushort NumberOutputValueCaps;
        public ushort NumberOutputDataIndices;
        public ushort NumberFeatureButtonCaps;
        public ushort NumberFeatureValueCaps;
        public ushort NumberFeatureDataIndices;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct HIDP_VALUE_CAPS
    {
        public ushort UsagePage;
        public byte ReportID;
        [MarshalAs(UnmanagedType.U1)] public bool IsAlias;
        public ushort BitField;
        public ushort LinkCollection;
        public ushort LinkUsage;
        public ushort LinkUsagePage;
        [MarshalAs(UnmanagedType.U1)] public bool IsRange;
        [MarshalAs(UnmanagedType.U1)] public bool IsStringRange;
        [MarshalAs(UnmanagedType.U1)] public bool IsDesignatorRange;
        [MarshalAs(UnmanagedType.U1)] public bool IsAbsolute;
        [MarshalAs(UnmanagedType.U1)] public bool HasNull;
        public byte Reserved;
        public ushort BitSize;
        public ushort ReportCount;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 5)] public ushort[] Reserved2;
        public uint UnitsExp;
        public uint Units;
        public int LogicalMin;
        public int LogicalMax;
        public int PhysicalMin;
        public int PhysicalMax;
        public ushort UsageMin;
        public ushort UsageMax;
        public ushort StringMin;
        public ushort StringMax;
        public ushort DesignatorMin;
        public ushort DesignatorMax;
        public ushort DataIndexMin;
        public ushort DataIndexMax;
    }

    [DllImport("hid.dll")] public static extern void HidD_GetHidGuid(out Guid guid);
    [DllImport("hid.dll")] public static extern bool HidD_GetAttributes(IntPtr h, ref HIDD_ATTRIBUTES a);
    [DllImport("hid.dll")] public static extern bool HidD_GetPreparsedData(IntPtr h, out IntPtr pp);
    [DllImport("hid.dll")] public static extern bool HidD_FreePreparsedData(IntPtr pp);
    [DllImport("hid.dll")] public static extern bool HidD_GetProductString(IntPtr h, byte[] buf, int len);
    [DllImport("hid.dll")] public static extern bool HidD_GetManufacturerString(IntPtr h, byte[] buf, int len);
    [DllImport("hid.dll")] public static extern bool HidD_GetSerialNumberString(IntPtr h, byte[] buf, int len);
    [DllImport("hid.dll")] public static extern bool HidD_GetFeature(IntPtr h, byte[] buf, int len);
    [DllImport("hid.dll")] public static extern bool HidD_GetInputReport(IntPtr h, byte[] buf, int len);
    [DllImport("hid.dll")] public static extern int HidP_GetCaps(IntPtr pp, ref HIDP_CAPS caps);
    [DllImport("hid.dll")] public static extern int HidP_GetValueCaps(int type, [Out] HIDP_VALUE_CAPS[] caps, ref ushort len, IntPtr pp);
    [DllImport("hid.dll")] public static extern int HidP_GetUsageValue(int type, ushort usagePage, ushort linkCollection,
        ushort usage, out uint value, IntPtr pp, byte[] report, int reportLength);

    [DllImport("setupapi.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SetupDiGetClassDevs(ref Guid guid, IntPtr enumerator, IntPtr hwnd, int flags);
    [DllImport("setupapi.dll")]
    public static extern bool SetupDiEnumDeviceInterfaces(IntPtr set, IntPtr devInfo, ref Guid guid, int index, ref SP_DEVICE_INTERFACE_DATA data);
    [DllImport("setupapi.dll", CharSet = CharSet.Unicode)]
    public static extern bool SetupDiGetDeviceInterfaceDetail(IntPtr set, ref SP_DEVICE_INTERFACE_DATA data,
        IntPtr detail, int detailSize, out int required, IntPtr devInfo);
    [DllImport("setupapi.dll")]
    public static extern bool SetupDiDestroyDeviceInfoList(IntPtr set);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr CreateFile(string name, uint access, uint share, IntPtr sec, uint disp, uint flags, IntPtr tmpl);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr h);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool DeviceIoControl(IntPtr h, uint code, IntPtr inBuf, int inSize,
        byte[] outBuf, int outSize, out int returned, IntPtr overlapped);

    public static string Wide(byte[] b)
    {
        string s = Encoding.Unicode.GetString(b);
        int z = s.IndexOf('\0');
        if (z >= 0) s = s.Substring(0, z);
        return s.Trim();
    }

    public static uint GetUsage(IntPtr pp, int type, HIDP_VALUE_CAPS c, byte[] report)
    {
        uint v;
        ushort usage = c.UsageMin;
        int rc = HidP_GetUsageValue(type, c.UsagePage, 0, usage, out v, pp, report, report.Length);
        if (rc != 0x00110000) { Console.WriteLine("      [usage get failed rc=0x{0:X8} for page=0x{1:X4} usage=0x{2:X4}]", rc, c.UsagePage, usage); return 0xFFFFFFFF; }
        return v;
    }

    public static IntPtr Open(string path)
    {
        IntPtr h = CreateFile(path, GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE,
            IntPtr.Zero, OPEN_EXISTING, 0, IntPtr.Zero);
        if (h == new IntPtr(-1))
            h = CreateFile(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE,
                IntPtr.Zero, OPEN_EXISTING, 0, IntPtr.Zero);
        return h;
    }

    public static void Dump(string path)
    {
        IntPtr h = Open(path);
        if (h == new IntPtr(-1)) { Console.WriteLine("  [open failed, err=" + Marshal.GetLastWin32Error() + "]"); return; }

        HIDD_ATTRIBUTES a = new HIDD_ATTRIBUTES();
        a.Size = Marshal.SizeOf(a);
        HidD_GetAttributes(h, ref a);
        if (a.VendorID != 0x051D) { CloseHandle(h); return; }

        byte[] sb = new byte[512];
        HidD_GetManufacturerString(h, sb, sb.Length);
        string mfr = Wide(sb);
        sb = new byte[512]; HidD_GetProductString(h, sb, sb.Length); string prod = Wide(sb);
        sb = new byte[512]; HidD_GetSerialNumberString(h, sb, sb.Length); string serial = Wide(sb);

        Console.WriteLine("=== APC HID UPS ===");
        Console.WriteLine("Path:    " + path);
        Console.WriteLine("VID/PID: {0:X4}:{1:X4}  version {2:X4}", a.VendorID, a.ProductID, a.VersionNumber);
        Console.WriteLine("Mfr:     " + mfr);
        Console.WriteLine("Product: " + prod);
        Console.WriteLine("Serial:  " + serial);

        IntPtr pp;
        if (!HidD_GetPreparsedData(h, out pp)) { Console.WriteLine("  [no preparsed data]"); CloseHandle(h); return; }
        HIDP_CAPS caps = new HIDP_CAPS();
        int cr = HidP_GetCaps(pp, ref caps);
        Console.WriteLine("Caps rc={0} UsagePage=0x{1:X4} Usage=0x{2:X4} In={3} Out={4} Feature={5}",
            cr, caps.UsagePage, caps.Usage, caps.InputReportByteLength, caps.OutputReportByteLength, caps.FeatureReportByteLength);
        Console.WriteLine("value cap counts: input={0} output={1} feature={2}",
            caps.NumberInputValueCaps, caps.NumberOutputValueCaps, caps.NumberFeatureValueCaps);

        // Raw HID report descriptor (IOCTL_HID_GET_REPORT_DESCRIPTOR)
        byte[] desc = new byte[4096];
        int got;
        if (DeviceIoControl(h, 0x000B0000, IntPtr.Zero, 0, desc, desc.Length, out got, IntPtr.Zero))
            Console.WriteLine("REPORT_DESCRIPTOR ({0} bytes): {1}", got, BitConverter.ToString(desc, 0, got));
        else
            Console.WriteLine("report descriptor read failed, err=" + Marshal.GetLastWin32Error());

        for (int type = 0; type <= 2; type++)
        {
            int count = type == HidP_Input ? caps.NumberInputValueCaps : (type == HidP_Output ? caps.NumberOutputValueCaps : caps.NumberFeatureValueCaps);
            if (count <= 0) continue;
            HIDP_VALUE_CAPS[] arr = new HIDP_VALUE_CAPS[count];
            ushort len = (ushort)count;
            int rc = HidP_GetValueCaps(type, arr, ref len, pp);
            string tn = type == HidP_Input ? "INPUT" : (type == HidP_Output ? "OUTPUT" : "FEATURE");
            Console.WriteLine("--- {0} value caps (rc={1}, n={2}) ---", tn, rc, len);

            // group report IDs
            System.Collections.Generic.List<byte> rids = new System.Collections.Generic.List<byte>();
            for (int i = 0; i < len; i++) if (!rids.Contains(arr[i].ReportID)) rids.Add(arr[i].ReportID);

            int repLen = type == HidP_Input ? caps.InputReportByteLength : (type == HidP_Output ? caps.OutputReportByteLength : caps.FeatureReportByteLength);
            foreach (byte rid in rids)
            {
                byte[] buf = new byte[repLen];
                buf[0] = rid;
                bool ok;
                if (type == HidP_Feature) ok = HidD_GetFeature(h, buf, buf.Length);
                else if (type == HidP_Input) ok = HidD_GetInputReport(h, buf, buf.Length);
                else ok = false;
                Console.WriteLine(" report 0x{0:X2}: read={1} raw={2}", rid, ok, BitConverter.ToString(buf));
                if (!ok) continue;

                for (int i = 0; i < len; i++)
                {
                    HIDP_VALUE_CAPS c = arr[i];
                    if (c.ReportID != rid || c.IsRange) continue;
                    uint v = GetUsage(pp, type, c, buf);
                    if (v == 0xFFFFFFFF) continue;
                    Console.WriteLine("    page=0x{0:X4} usage=0x{1:X2} size={2} log=[{3},{4}] unit=0x{5:X} exp={6} => {7}",
                        c.UsagePage, c.UsageMin, c.BitSize, c.LogicalMin, c.LogicalMax, c.Units, c.UnitsExp, v);
                }
            }
        }

        HidD_FreePreparsedData(pp);
        CloseHandle(h);
    }
}
'@

$guid = [Guid]::Empty
[HidProbe]::HidD_GetHidGuid([ref]$guid)
$set = [HidProbe]::SetupDiGetClassDevs([ref]$guid, [IntPtr]::Zero, [IntPtr]::Zero, 0x12)
if ($set -eq [IntPtr]::Zero) { throw "SetupDiGetClassDevs failed" }

$found = 0
for ($i = 0; ; $i++) {
    $did = New-Object HidProbe+SP_DEVICE_INTERFACE_DATA
    $did.cbSize = [Runtime.InteropServices.Marshal]::SizeOf($did)
    if (-not [HidProbe]::SetupDiEnumDeviceInterfaces($set, [IntPtr]::Zero, [ref]$guid, $i, [ref]$did)) { break }

    $req = 0
    [void][HidProbe]::SetupDiGetDeviceInterfaceDetail($set, [ref]$did, [IntPtr]::Zero, 0, [ref]$req, [IntPtr]::Zero)
    $detail = [Runtime.InteropServices.Marshal]::AllocHGlobal($req)
    try {
        # cbSize is 8 on x64, 6 on x86
        $cb = if ([IntPtr]::Size -eq 8) { 8 } else { 6 }
        [Runtime.InteropServices.Marshal]::WriteInt32($detail, $cb)
        if ([HidProbe]::SetupDiGetDeviceInterfaceDetail($set, [ref]$did, $detail, $req, [ref]$req, [IntPtr]::Zero)) {
            $path = [Runtime.InteropServices.Marshal]::PtrToStringUni([IntPtr]::Add($detail, 4))
            [HidProbe]::Dump($path)
            $found++
        }
    } finally {
        [Runtime.InteropServices.Marshal]::FreeHGlobal($detail)
    }
}
[void][HidProbe]::SetupDiDestroyDeviceInfoList($set)
if ($found -eq 0) { Write-Host "No APC HID device found." }
