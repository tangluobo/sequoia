import com.tangluobo.tomato.ssh.zmodem.ZModem;
import com.tangluobo.tomato.ssh.zmodem.util.CustomFile;
import com.tangluobo.tomato.ssh.zmodem.util.FileAdapter;
import java.io.BufferedInputStream;
import java.io.File;
import java.util.ArrayList;
import java.util.List;

/** Test-only stdio adapter around the untouched Java implementation. */
public final class LegacyZmodemPeer {
    // The protocol accepts a FileAdapter. Use directly visible writes so
    // its resume check observes the current length before reopening a file.
    // CustomFile uses BufferedOutputStream but checks length before flushing.
    private static final class VisibleFile extends CustomFile {
        private final File file;
        VisibleFile(File file){super(file);this.file=file;}
        @Override public FileAdapter getChild(String name){return new VisibleFile(new File(file,name));}
        @Override public java.io.OutputStream getOutputStream(boolean append)throws java.io.IOException{return new java.io.FileOutputStream(file,append);}
    }
    public static void main(String[] args) throws Exception {
        // The library is the application side: emulate the remote rz/sz
        // announcement before handing control to its original state machine.
        String traceDir=System.getenv("TOMATO_TEST_ZMODEM_TRACE");
        java.io.PrintWriter trace=traceDir==null?null:new java.io.PrintWriter(new java.io.FileOutputStream(new File(traceDir,"peer-"+args[0]+"-"+ProcessHandle.current().pid()+".log")),true);
        java.io.InputStream input=new java.io.FilterInputStream(System.in){@Override public int read(byte[] bytes,int offset,int count)throws java.io.IOException{int n=in.read(bytes,offset,count);if(trace!=null&&n>0)trace.println("IN "+n+" "+java.util.HexFormat.of().formatHex(bytes,offset,offset+Math.min(n,64)));return n;}};
        java.io.OutputStream output=new java.io.FilterOutputStream(System.out){@Override public void write(byte[] bytes,int offset,int count)throws java.io.IOException{out.write(bytes,offset,count);if(trace!=null)trace.println("OUT "+count+" "+java.util.HexFormat.of().formatHex(bytes,offset,offset+Math.min(count,64)));}};
        var packets=new com.tangluobo.tomato.ssh.zmodem.zm.io.ZMPacketOutputStream(output);
        var format=com.tangluobo.tomato.ssh.zmodem.xfer.zm.packet.Format.HEX;
        var type=args[0].equals("send") ? com.tangluobo.tomato.ssh.zmodem.xfer.zm.util.ZModemCharacter.ZRQINIT : com.tangluobo.tomato.ssh.zmodem.xfer.zm.util.ZModemCharacter.ZRINIT;
        packets.write(new com.tangluobo.tomato.ssh.zmodem.xfer.zm.packet.Header(format,type));
        System.out.flush();
        ZModem modem = new ZModem(new BufferedInputStream(input), output);
        if (args[0].equals("send")) {
            List<FileAdapter> files = new ArrayList<>();
            for (int i = 1; i < args.length; i++) files.add(new CustomFile(new File(args[i])));
            modem.send(() -> files);
        } else if (args[0].equals("receive")) {
            modem.receive(() -> new VisibleFile(new File(args[1])));
        } else {
            throw new IllegalArgumentException("send <files...> | receive <directory>");
        }
    }
}
