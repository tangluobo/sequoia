import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import javax.crypto.*;
import javax.crypto.spec.*;
import com.google.gson.*;
/** Invokes the current original Tencent signing helpers only; no cloud HTTP calls. */
public class CloudSigningOracle {
 public static void main(String[] args)throws Exception {
  var original=Class.forName("com.tangluobo.tomato.module.connect.service.TencentCloudService");
  var hash=original.getDeclaredMethod("sha256",String.class);hash.setAccessible(true);
  var hmac=original.getDeclaredMethod("hmac",byte[].class,String.class);hmac.setAccessible(true);
  String id="AKIDEXAMPLE",secret="fixture-secret-中文",service="cvm",host="cvm.tencentcloudapi.com",date="2019-02-25";
  long timestamp=1551113065;
  String body="{\"Limit\":1,\"Filters\":[{\"Values\":[\"未命名\"],\"Name\":\"instance-name\"}]}";
  String canonical="POST\n/\n\ncontent-type:application/json; charset=utf-8\nhost:"+host+"\n\ncontent-type;host\n"+hash.invoke(null,body);
  String scope=date+"/"+service+"/tc3_request";
  String toSign="TC3-HMAC-SHA256\n"+timestamp+"\n"+scope+"\n"+hash.invoke(null,canonical);
  byte[] key=(byte[])hmac.invoke(null,("TC3"+secret).getBytes(StandardCharsets.UTF_8),date);
  key=(byte[])hmac.invoke(null,key,service);key=(byte[])hmac.invoke(null,key,"tc3_request");
  String auth="TC3-HMAC-SHA256 Credential="+id+"/"+scope+", SignedHeaders=content-type;host, Signature="+HexFormat.of().formatHex((byte[])hmac.invoke(null,key,toSign));
  var result=new LinkedHashMap<String,Object>();result.put("id",id);result.put("secret",secret);result.put("service",service);result.put("host",host);result.put("date",date);result.put("timestamp",timestamp);result.put("body",body);result.put("authorization",auth);
  Files.writeString(Path.of(args[0]),new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(result));
 }
}