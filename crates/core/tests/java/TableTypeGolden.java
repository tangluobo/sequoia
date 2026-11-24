import com.google.gson.*;
import com.tangluobo.tomato.module.connect.*;
import java.nio.file.*;
/** Pure original version/type selection; no configuration, network or UI. */
public class TableTypeGolden {
 public static void main(String[] args)throws Exception {
  var versions=new String[]{null,"","5.6.51","5.7.0","8.4.11","13.9","14.0","21c","19c","PostgreSQL 17.11","invalid","99999999999999999999999","."};
  var rows=new JsonArray();
  for(var kind:new ConnectType[]{ConnectType.MYSQL,ConnectType.POSTGRESQL,ConnectType.ORACLE,ConnectType.HIVE})for(var version:versions){
   var row=new JsonObject();row.addProperty("kind",kind.name());row.add("version",version==null?JsonNull.INSTANCE:new JsonPrimitive(version));
   try{row.add("types",new Gson().toJsonTree(DataTypeProvider.getDataTypes(kind,version)));}
   catch(Throwable error){row.addProperty("error",error.getClass().getSimpleName());}
   rows.add(row);
  }
  Files.writeString(Path.of(args[0]),new GsonBuilder().serializeNulls().setPrettyPrinting().create().toJson(rows));
 }
}
