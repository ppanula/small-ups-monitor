#requires -Version 5
# Read-only dump of APC HID value capabilities (VID 051D), no report reads.
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;

public static class HidCaps
{
    [StructLayout(LayoutKind.Sequential)]
    public struct SP_DEVICE_INTERFACE_DATA { public int cbSize; public Guid InterfaceClassGuid; public int Flags; public IntPtr Reserved; }

    [StructLayout(LayoutKind.Sequential)]
    public struct HIDD_ATTRIBUTES { public int Size; public ushort VendorID; public ushort ProductID; public ushort VersionNumber; }

    [StructLayout(LayoutKind.Sequential)]
    public struct HIDP_LINK_COLLECTION_NODE
    {
        public ushort LinkUsage; public ushort LinkUsagePage; public ushort Parent; public ushort NumberOfChildren;
        public ushort NextSibling; public ushort FirstChild; public uint CollectionType; public IntPtr UserContext;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct HIDP_CAPS
    {
        public ushort Usage; public ushort UsagePage;
        public ushort InputReportByteLength; public ushort OutputReportByteLength; public ushort FeatureReportByteLength;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 17)] public ushort[] Reserved;
        public ushort NumberLinkCollectionNodes;
        public ushort NumberInputButtonCaps; public ushort NumberInputValueCaps; public ushort NumberInputDataIndices;
        public ushort NumberOutputButtonCaps; public ushort NumberOutputValueCaps; public ushort NumberOutputDataIndices;
        public ushort NumberFeatureButtonCaps; public ushort NumberFeatureValueCaps; public ushort NumberFeatureDataIndices;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct HIDP_VALUE_CAPS
    {
        public ushort UsagePage; public byte ReportID;
        [MarshalAs(UnmanagedType.U1)] public bool IsAlias;
        public ushort BitField; public ushort LinkCollection; public ushort LinkUsage; public ushort LinkUsagePage;
        [MarshalAs(UnmanagedType.U1)] public bool IsRange;
        [MarshalAs(UnmanagedType.U1)] public bool IsStringRange;
        [MarshalAs(UnmanagedType.U1)] public bool IsDesignatorRange;
        [MarshalAs(UnmanagedType.U1)] public bool IsAbsolute;
        [MarshalAs(UnmanagedType.U1)] public bool HasNull;
        public byte Reserved; public ushort BitSize; public ushort ReportCount;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst = 5)] public ushort[] Reserved2;
        public uint UnitsExp; public uint Units;
        public int LogicalMin; public int LogicalMax; public int PhysicalMin; public int PhysicalMax;
        public ushort UsageMin; public ushort UsageMax; public ushort StringMin; public ushort StringMax;
        public ushort DesignatorMin; public ushort DesignatorMax; public ushort DataIndexMin; public ushort DataIndexMax;
    }

    [DllImport("hid.dll")] public static extern void HidD_GetHidGuid(out Guid g);
    [DllImport("hid.dll")] public static extern bool HidD_GetAttributes(IntPtr h, ref HIDD_ATTRIBUTES a);
    [DllImport("hid.dll")] public static extern bool HidD_GetPreparsedData(IntPtr h, out IntPtr pp);
    [DllImport("hid.dll")] public static extern bool HidD_FreePreparsedData(IntPtr pp);
    [DllImport("hid.dll")] public static extern bool HidD_GetProductString(IntPtr h, byte[] b, int n);
    [DllImport("hid.dll")] public static extern int HidP_GetCaps(IntPtr pp, ref HIDP_CAPS c);
    [DllImport("hid.dll")] public static extern int HidP_GetValueCaps(int type, [In, Out] HIDP_VALUE_CAPS[] caps, ref ushort len, IntPtr pp);
    [DllImport("hid.dll")] public static extern int HidP_GetLinkCollectionNodes([In, Out] HIDP_LINK_COLLECTION_NODE[] nodes, ref uint len, IntPtr pp);

    [DllImport("setupapi.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SetupDiGetClassDevs(ref Guid g, IntPtr e, IntPtr w, int f);
    [DllImport("setupapi.dll")] public static extern bool SetupDiEnumDeviceInterfaces(IntPtr s, IntPtr d, ref Guid g, int i, ref SP_DEVICE_INTERFACE_DATA a);
    [DllImport("setupapi.dll", CharSet = CharSet.Unicode)] public static extern bool SetupDiGetDeviceInterfaceDetail(IntPtr s, ref SP_DEVICE_INTERFACE_DATA a, IntPtr b, int n, out int r, IntPtr d);
    [DllImport("setupapi.dll")] public static extern bool SetupDiDestroyDeviceInfoList(IntPtr s);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] public static extern IntPtr CreateFile(string n, uint a, uint s, IntPtr sec, uint d, uint f, IntPtr t);
    [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);

    public static string Wide(byte[] b)
    {
        string s = Encoding.Unicode.GetString(b);
        int z = s.IndexOf('\0'); if (z >= 0) s = s.Substring(0, z);
        return s.Trim();
    }

    public static void Dump(string path)
    {
        IntPtr h = CreateFile(path, 0x80000000 | 0x40000000, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
        if (h == new IntPtr(-1))
            h = CreateFile(path, 0x80000000, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
        if (h == new IntPtr(-1)) return;

        HIDD_ATTRIBUTES a = new HIDD_ATTRIBUTES(); a.Size = Marshal.SizeOf(a);
        HidD_GetAttributes(h, ref a);
        if (a.VendorID != 0x051D) { CloseHandle(h); return; }

        byte[] sb = new byte[512];
        HidD_GetProductString(h, sb, sb.Length);
        Console.WriteLine("DEVICE " + Wide(sb) + "  " + a.VendorID.ToString("X4") + ":" + a.ProductID.ToString("X4"));

        IntPtr pp;
        if (!HidD_GetPreparsedData(h, out pp)) { CloseHandle(h); return; }
        HIDP_CAPS c = new HIDP_CAPS();
        HidP_GetCaps(pp, ref c);
        Console.WriteLine("CAPS page=0x{0:X4} usage=0x{1:X4} in={2} feature={3} counts in/out/feat={4}/{5}/{6}",
            c.UsagePage, c.Usage, c.InputReportByteLength, c.FeatureReportByteLength,
            c.NumberInputValueCaps, c.NumberOutputValueCaps, c.NumberFeatureValueCaps);

        if (c.NumberLinkCollectionNodes > 0)
        {
            HIDP_LINK_COLLECTION_NODE[] nodes = new HIDP_LINK_COLLECTION_NODE[c.NumberLinkCollectionNodes];
            uint nlen = c.NumberLinkCollectionNodes;
            int nrc = HidP_GetLinkCollectionNodes(nodes, ref nlen, pp);
            Console.WriteLine("--- link collections rc=0x{0:X8} n={1} ---", nrc, nlen);
            for (int i = 0; i < nlen; i++)
            {
                HIDP_LINK_COLLECTION_NODE n = nodes[i];
                Console.WriteLine("  [{0}] page=0x{1:X4} usage=0x{2:X4} parent={3} children={4} next={5} first={6} type={7}",
                    i, n.LinkUsagePage, n.LinkUsage, n.Parent, n.NumberOfChildren, n.NextSibling, n.FirstChild, n.CollectionType & 0xFF);
            }
        }

        for (int type = 0; type <= 2; type++)
        {
            int count = type == 0 ? c.NumberInputValueCaps : (type == 1 ? c.NumberOutputValueCaps : c.NumberFeatureValueCaps);
            if (count == 0) continue;
            HIDP_VALUE_CAPS[] arr = new HIDP_VALUE_CAPS[count];
            ushort len = (ushort)count;
            int rc = HidP_GetValueCaps(type, arr, ref len, pp);
            Console.WriteLine("--- type {0} rc={1} returned={2} sizeof={3} ---", type, rc, len, Marshal.SizeOf(typeof(HIDP_VALUE_CAPS)));
            for (int i = 0; i < len; i++)
            {
                HIDP_VALUE_CAPS v = arr[i];
                Console.WriteLine("  rid=0x{0:X2} page=0x{1:X4} usage=0x{2:X4}{3} size={4} count={5} log=[{6},{7}] unit=0x{8:X} exp={9} link={10} abs={11}",
                    v.ReportID, v.UsagePage, v.UsageMin, v.IsRange ? ("-0x" + v.UsageMax.ToString("X4")) : "",
                    v.BitSize, v.ReportCount, v.LogicalMin, v.LogicalMax, v.Units, v.UnitsExp, v.LinkCollection, v.IsAbsolute);
            }
        }
        HidD_FreePreparsedData(pp);
        CloseHandle(h);
    }
}
'@

$g = [Guid]::Empty
[HidCaps]::HidD_GetHidGuid([ref]$g)
$set = [HidCaps]::SetupDiGetClassDevs([ref]$g, [IntPtr]::Zero, [IntPtr]::Zero, 0x12)
for ($i = 0; ; $i++) {
    $did = New-Object HidCaps+SP_DEVICE_INTERFACE_DATA
    $did.cbSize = [Runtime.InteropServices.Marshal]::SizeOf($did)
    if (-not [HidCaps]::SetupDiEnumDeviceInterfaces($set, [IntPtr]::Zero, [ref]$g, $i, [ref]$did)) { break }
    $req = 0
    [void][HidCaps]::SetupDiGetDeviceInterfaceDetail($set, [ref]$did, [IntPtr]::Zero, 0, [ref]$req, [IntPtr]::Zero)
    $d = [Runtime.InteropServices.Marshal]::AllocHGlobal($req)
    try {
        [Runtime.InteropServices.Marshal]::WriteInt32($d, $(if ([IntPtr]::Size -eq 8) { 8 } else { 6 }))
        if ([HidCaps]::SetupDiGetDeviceInterfaceDetail($set, [ref]$did, $d, $req, [ref]$req, [IntPtr]::Zero)) {
            [HidCaps]::Dump([Runtime.InteropServices.Marshal]::PtrToStringUni([IntPtr]::Add($d, 4)))
        }
    } finally { [Runtime.InteropServices.Marshal]::FreeHGlobal($d) }
}
[void][HidCaps]::SetupDiDestroyDeviceInfoList($set)
