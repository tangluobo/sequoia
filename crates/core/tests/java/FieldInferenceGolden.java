import com.google.gson.*;
import com.tangluobo.tomato.module.connect.*;
import java.nio.file.*;
import java.util.*;
/** Original field-name inference only; no UI, connection, config or clipboard. */
public class FieldInferenceGolden {
 public static void main(String[] args)throws Exception {
  var names=new String[]{null,""," ","\u2003","\u00a0","id","userID","UUID","userGuid","guid_id","createdAt","updated_at","eventDateTime","birthday","eventTime","create_date_time","datatime","timestamp","at","at_created","created_at","isActive","hasValue","canRead","island","has_amount","content_flag","amount_count","price","userAge","version","description","imageBytes","payload_json","sourceJsonb","name","JSONDataID","XMLUserName","唯一标识","用户编号","主键名称","创建时间","时间戳","生日","日期","时间","是否有效","中文名称","用户_日期_time","日期时间","GUID编号","\u00a0id\u00a0","İd","ID\u2003COUNT","sha256Hash","emoji😀","foo_bar_baz"};
  var gson=new GsonBuilder().serializeNulls().setPrettyPrinting().create();var rows=new JsonArray();
  for(var kind:new ConnectType[]{ConnectType.MYSQL,ConnectType.POSTGRESQL,ConnectType.ORACLE,ConnectType.HIVE})for(boolean json:new boolean[]{false,true})for(var name:names){
   var available=json?List.of("jSoN"):List.<String>of();
   var row=new JsonObject();row.addProperty("kind",kind.name());row.add("field",name==null?JsonNull.INSTANCE:new JsonPrimitive(name));row.add("available",gson.toJsonTree(available));row.add("expected",gson.toJsonTree(FieldTypeInference.infer(kind,name,available)));rows.add(row);
  }
  Files.writeString(Path.of(args[0]),gson.toJson(rows));
 }
}
