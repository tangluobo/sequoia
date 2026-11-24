// Independent JCA implementation of MS-RDPBCGR 5.3, only for deterministic QA.
import java.security.*;
import java.util.*;
import javax.crypto.Cipher;
import javax.crypto.spec.SecretKeySpec;
public final class RdpCryptoReference {
    static byte[] digest(String name,byte[]...data)throws Exception{var h=MessageDigest.getInstance(name);for(var p:data)h.update(p);return h.digest();}
    static byte[] join(byte[]...values){var out=new java.io.ByteArrayOutputStream();for(var p:values)out.writeBytes(p);return out.toByteArray();}
    static byte[] part(byte[]data,int a,int b){return Arrays.copyOfRange(data,a,b);}
    static byte[] repeat(int value,int length){var p=new byte[length];Arrays.fill(p,(byte)value);return p;}
    static byte[] number(int n){return new byte[]{(byte)n,(byte)(n>>8),(byte)(n>>16),(byte)(n>>24)};}
    static byte[] salt(byte[]s,String label,byte[]c,byte[]v)throws Exception{return digest("MD5",s,digest("SHA-1",label.getBytes(java.nio.charset.StandardCharsets.US_ASCII),s,c,v));}
    static byte[] reduce(byte[]key,int bits){key=part(key,0,bits==128?16:8);if(bits<128)key[0]=(byte)0xd1;if(bits==40){key[1]=0x26;key[2]=(byte)0x9e;}return key;}
    static Cipher rc4(byte[]key)throws Exception{var c=Cipher.getInstance("RC4");c.init(Cipher.ENCRYPT_MODE,new SecretKeySpec(key,"RC4"));return c;}
    static String reference(int bits,boolean receive)throws Exception {
        var client=new byte[32];var server=new byte[32];for(int i=0;i<32;i++){client[i]=(byte)i;server[i]=(byte)(i+32);}
        var pre=join(part(client,0,24),part(server,0,24));
        var master=join(salt(pre,"A",client,server),salt(pre,"BB",client,server),salt(pre,"CCC",client,server));
        var blob=join(salt(master,"X",client,server),salt(master,"YY",client,server),salt(master,"ZZZ",client,server));
        var mac=reduce(part(blob,0,16),bits);var initial=reduce(digest("MD5",part(blob,receive?16:32,receive?32:48),client,server),bits);
        var key=initial.clone();var cipher=rc4(key);var aggregate=MessageDigest.getInstance("SHA-256");
        for(int count=0;count<8195;count++) {
            if(count>0 && count%4096==0){
                var sha=digest("SHA-1",initial,repeat(0x36,40),key);
                key=part(digest("MD5",initial,repeat(0x5c,48),sha),0,key.length);
                key=reduce(rc4(key).doFinal(key),bits);cipher=rc4(key);
            }
            var data=new byte[127+count%4096];for(int i=0;i<data.length;i++)data[i]=(byte)(i*17+count);
            var sha=digest("SHA-1",mac,repeat(0x36,40),number(data.length),data,count%2==0?number(count):new byte[0]);
            aggregate.update(part(digest("MD5",mac,repeat(0x5c,48),sha),0,8));aggregate.update(cipher.update(data));
        }
        return HexFormat.of().formatHex(aggregate.digest());
    }
    public static void main(String[]args)throws Exception{
        for(int bits:new int[]{40,56,128})System.out.println(bits+" "+reference(bits,false)+" "+reference(bits,true));
    }
}
