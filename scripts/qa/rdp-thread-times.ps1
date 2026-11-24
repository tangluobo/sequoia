# CPU counters only: no UI automation, stack inspection or remote process access.
if(!('TomatoRdpThreadTimes' -as [type])){
 Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public sealed class TomatoRdpThreadSample {
 public long CreatedTicks;
 public long CpuTicks;
 public string Name;
}
public static class TomatoRdpThreadTimes {
 [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
 [DllImport("kernel32.dll")] static extern uint GetProcessIdOfThread(IntPtr thread);
 [DllImport("kernel32.dll")] static extern bool GetThreadTimes(IntPtr thread, out long created, out long exited, out long kernel, out long user);
 [DllImport("kernel32.dll")] static extern int GetThreadDescription(IntPtr thread, out IntPtr name);
 [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr memory);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
 public static TomatoRdpThreadSample Read(uint processId, uint threadId) {
  IntPtr thread=OpenThread(0x0800,false,threadId);
  if(thread==IntPtr.Zero) return null;
  try {
   if(GetProcessIdOfThread(thread)!=processId) return null;
   long created,exited,kernel,user;
   if(!GetThreadTimes(thread,out created,out exited,out kernel,out user)) return null;
   string name="";
   IntPtr pointer;
   if(GetThreadDescription(thread,out pointer)>=0 && pointer!=IntPtr.Zero) {
    try { name=Marshal.PtrToStringUni(pointer) ?? ""; } finally { LocalFree(pointer); }
   }
   return new TomatoRdpThreadSample { CreatedTicks=created,CpuTicks=kernel+user,Name=name };
  } finally { CloseHandle(thread); }
 }
}
'@
}
function Get-OwnedRdpThreadTimes([Diagnostics.Process]$Application,[string]$ExpectedExecutable){
 if($Application.HasExited -or ![string]::Equals($Application.MainModule.FileName,$ExpectedExecutable,[StringComparison]::OrdinalIgnoreCase)){throw 'Owned GUI identity changed before thread sampling'}
 $result=@{}
 foreach($thread in $Application.Threads){
  $sample=[TomatoRdpThreadTimes]::Read([uint32]$Application.Id,[uint32]$thread.Id)
  if($null -ne $sample){$result[[int]$thread.Id]=$sample}
 }
 return $result
}
