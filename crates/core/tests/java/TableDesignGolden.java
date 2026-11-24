import com.google.gson.*;
import com.tangluobo.tomato.module.connect.*;
import com.tangluobo.tomato.module.connect.service.DatabaseService;
import java.nio.file.*;
import java.util.*;
/** Pure original DDL generation only. No connection, filesystem config or clipboard. */
public class TableDesignGolden {
 record Example(String name,ConnectType kind,String database,String schema,String table,
                List<Map<String,String>> columns,Map<String,String> options,String comment){}
 static Map<String,String> column(String... pairs) {
  var result=new LinkedHashMap<String,String>();
  for(int i=0;i<pairs.length;i+=2)result.put(pairs[i],pairs[i+1]);
  return result;
 }
 public static void main(String[] args)throws Exception {
  Locale.setDefault(Locale.US);
  var basic=List.of(
   column("字段名","id","类型","BIGINT","长度","20","非空","是","主键","是","自增","是"),
   column("字段名","name","类型","VARCHAR","长度","64","默认值","O'Reilly","注释","名称 '备注'"),
   column("字段名","amount","类型","DECIMAL","长度","12","小数点","2","无符号","是","填充零","是"));
  var options=Map.of("引擎","InnoDB","字符集","utf8mb4","排序规则","utf8mb4_0900_ai_ci");
  var cases=new ArrayList<Example>();
  cases.add(new Example("mysql-basic",ConnectType.MYSQL,"sample_db",null,"订单",basic,options,"订单 '备注'"));
  cases.add(new Example("postgres-basic",ConnectType.POSTGRESQL,"actual_db","analytics","orders",basic,options,"订单 '备注'"));
  cases.add(new Example("postgres-schema-fallback",ConnectType.POSTGRESQL,"public",null,"orders",basic,null,""));
  cases.add(new Example("oracle-basic",ConnectType.ORACLE,"APP",null,"orders",basic,null,"备注"));
  cases.add(new Example("hive-basic",ConnectType.HIVE,"sample_db",null,"orders",basic,null,"备注"));
  var precision=new ArrayList<Map<String,String>>();
  for(String type:new String[]{"DATE","DATETIME","TIMESTAMP","TIME","YEAR","TEXT","MEDIUMTEXT","BLOB","JSON","GEOMETRY","POINT","BOOL","BOOLEAN","INT","FLOAT","DOUBLE","REAL","NUMERIC","INTERVAL","ENUM","SET"})
   precision.add(column("字段名","column_"+precision.size(),"类型",type,"长度","20","小数点","3"));
  cases.add(new Example("mysql-type-size-rules",ConnectType.MYSQL,"sample_db",null,"types",precision,null,""));
  cases.add(new Example("mysql-zero-time-precision",ConnectType.MYSQL,"sample_db",null,"time_values",List.of(
   column("字段名","created","类型","TIMESTAMP","长度","26","小数点","0","默认值","current_timestamp"),
   column("字段名","nullable","类型","VARCHAR","长度","100","默认值","null"),
   column("字段名","localized","类型","VARCHAR","长度","20","字符集","utf8mb4","排序规则","utf8mb4_bin","非空","是")),null,""));
  cases.add(new Example("mysql-composite-key",ConnectType.MYSQL,"sample_db",null,"composite",List.of(
   column("字段名","left_id","类型","INT","主键","是"),column("字段名","right_id","类型","INT","主键","是"),
   column("字段名","  ","类型","VARCHAR")),Map.of(),""));
  cases.add(new Example("postgres-defaults-originally-ignored",ConnectType.POSTGRESQL,"actual_db","public","defaults",List.of(
   column("字段名","id","类型","INTEGER","默认值","42","自增","是","注释","原版此处不生成列注释")),null,""));
  var result=new JsonArray();var gson=new GsonBuilder().setPrettyPrinting().create();
  for(var example:cases){
   var config=new ConnectionConfig();config.setType(example.kind());
   var row=gson.toJsonTree(example).getAsJsonObject();
   row.addProperty("sql",DatabaseService.generateCreateTableSql(config,example.database(),example.schema(),example.table(),example.columns(),example.options(),example.comment()));
   result.add(row);
  }
  Files.writeString(Path.of(args[0]),gson.toJson(result));
 }
}
