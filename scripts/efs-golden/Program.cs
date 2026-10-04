// Offline reflection only: never constructs a serial port or QcdmManager.
using System.Reflection;
using System.Text.Json;
var assembly = Assembly.LoadFrom(Path.GetFullPath(args[0]));
Type T(string name) => assembly.GetType("EfsTools." + name, true)!;
object New(string name, params object[] values) => Activator.CreateInstance(T(name), BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic, null, values, null)!;
byte[] Request(string name, params object[] values) { var obj = New("Qualcomm.QcdmCommands.Requests." + name, values); return (byte[])obj.GetType().GetMethod("GetData")!.Invoke(obj, null)!; }
object Flags(int value) => Enum.ToObject(T("Qualcomm.QcdmCommands.Requests.Efs.EfsFileFlag"), value);
var fixtures = new SortedDictionary<string, string>();
void Add(string name, byte[] bytes) => fixtures[name] = Convert.ToHexString(bytes).ToLowerInvariant();
Add("password", Request("PasswordCommandRequest", "FFFFFFFFFFFFFFFF"));
Add("spc", Request("SpcCommandRequest", "000000"));
Add("hello", Request("Efs.EfsHelloCommandRequest"));
Add("open", Request("Efs.EfsOpenFileCommandRequest", "/nv/test", Flags(1 | 100), 777));
Add("put", Request("Efs.EfsPutItemFileCommandRequest", "/nv/test", Flags(1 | 1000 | 100 | 1000000), 777, new byte[]{0x7d,0x7e,1}));
Add("nvRead", Request("Nv.NvReadCommandRequest", (ushort)562));
Add("nvWrite", Request("Nv.NvWriteCommandRequest", (ushort)562, new byte[]{1}));
Add("nvEmpty", Request("Nv.NvWriteCommandRequest", (ushort)6789, Array.Empty<byte>()));
object EnumValue(string type, int value) => Enum.ToObject(T("Qualcomm.QcdmCommands.Requests."+type),value);
var logIds = Array.CreateInstance(T("Qualcomm.QcdmCommands.Base.LogId"),0);
var messageIds = Array.CreateInstance(T("Qualcomm.QcdmCommands.Base.MessageId"),0);
var tuple = Activator.CreateInstance(typeof(Tuple<,>).MakeGenericType(T("Qualcomm.QcdmCommands.Base.LogId"),T("Qualcomm.QcdmCommands.Base.LogId")),Enum.ToObject(T("Qualcomm.QcdmCommands.Base.LogId"),0),Enum.ToObject(T("Qualcomm.QcdmCommands.Base.LogId"),4099))!;
Add("logRanges",Request("LogConfigCommandRequest",EnumValue("LogConfigOperation",1),0,null!,null!));
Add("logMask",Request("LogConfigCommandRequest",EnumValue("LogConfigOperation",3),1,tuple,logIds));
Add("messageRanges",Request("ExtMessageConfigCommandRequest",EnumValue("ExtMessageConfigOperation",1),0,0,messageIds));
Add("messageMask",Request("ExtMessageConfigCommandRequest",EnumValue("ExtMessageConfigOperation",4),0,3,messageIds));
var errors = new SortedDictionary<string,int>();
foreach(var name in new[]{"NoEntry","DirectoryExist","FileExist","InvalidSequence"}) errors[name]=Convert.ToInt32(Enum.Parse(T("Qualcomm.QcdmCommands.QcdmEfsErrors"),name));
var encoder = T("Utils.HdlcEncoder").GetMethod("Encode")!;
Add("hdlc", (byte[])encoder.Invoke(null, new object[]{new byte[]{0x4b,0x13,0x7d,0x7e,0},false})!);
var sizes = new SortedDictionary<int, long>();
var factory = T("Items.ItemsFactory").GetMethod("SizeOfNvItem")!;
var ids = new HashSet<int>(new[]{71,562,880,881,882,1920,1921,3532,3533,4228,4229,4703,5596,6789,6790,6792,6849,7165});
if (args.Length > 1) foreach (var file in Directory.EnumerateFiles(args[1],"NvItem__*",SearchOption.AllDirectories)) {
    if (int.TryParse(Path.GetFileName(file).Substring(8),out var id)) ids.Add(id);
}
foreach (var id in ids) sizes[id]=(long)factory.Invoke(null,new object[]{id})!;
Console.WriteLine(JsonSerializer.Serialize(new { fixtures, sizes, errors },new JsonSerializerOptions{WriteIndented=true}));
