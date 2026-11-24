import com.google.gson.*;
import com.tangluobo.tomato.module.connect.service.AiSqlService;
import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import java.util.regex.Pattern;
/** Pure original AI text/protocol helpers. Never invokes generateSql or HTTP. */
public class AiSqlServiceGolden {
 static Object invoke(String name,Class<?>[] types,Object... args)throws Exception{var method=AiSqlService.class.getDeclaredMethod(name,types);method.setAccessible(true);return method.invoke(null,args);}
 public static void main(String[] args)throws Exception {
  var gson=new GsonBuilder().serializeNulls().setPrettyPrinting().disableHtmlEscaping().create();var output=new JsonObject();var one=new Class<?>[]{String.class};
  var urls=new JsonArray();for(String value:Arrays.asList(null,"","  ","https://api.example.invalid/v1","https://api.example.invalid/v1///","https://api.example.invalid/v1/chat/completions/","HTTPS://API.EXAMPLE.INVALID/CHAT/COMPLETIONS","http://localhost:11434/v1")){var item=new JsonObject();item.addProperty("input",value);item.addProperty("output",(String)invoke("chatCompletionsUrl",one,value));urls.add(item);}output.add("urls",urls);
  var patternField=AiSqlService.class.getDeclaredField("SQL_START");patternField.setAccessible(true);var pattern=(Pattern)patternField.get(null);var clean=new JsonArray();
  for(String value:Arrays.asList(null,""," SELECT 1; ","```sql\nSELECT 1;\n```","```SQL\r\nWITH x AS (SELECT 1) SELECT * FROM x\r\n```","```SELECT 1;```","SQL: SELECT 1;","sql:\tSHOW TABLES","ſQL: SELECT 1;","解释：SELECT 1;","-- 注释\n/* 说明 */ SELECT 1;","-- 注释\rSELECT 1;","/*未闭合 SELECT 1","selecté","selection","SELECT_1","WITH(SELECT 1)","MERGE INTO t","CALL p()","DESC t","DESCRIBE t","DROP TABLE t","DELETE FROM t","GRANT SELECT ON t","\ufeffSELECT 1;","\u2003SELECT 1;")){
   var item=new JsonObject();item.addProperty("input",value);String text=(String)invoke("cleanSql",one,value);item.addProperty("output",text);item.addProperty("sql",pattern.matcher(text).find());clean.add(item);
  }output.add("clean",clean);
  var systems=new JsonArray();for(String[] values:new String[][]{{null,null,null,null},{"MYSQL","app","public","TABLE users\n  id INT PRIMARY KEY\n  name VARCHAR(20)"},{" ","\t","\u2003",""},{"PostgreSQL","数据库","模式","TABLE 中文表\n  主键 BIGINT"},{"SQL","\u00a0","","\u2003"}}){var item=new JsonObject();item.add("input",gson.toJsonTree(values));item.addProperty("output",(String)invoke("buildSystemPrompt",new Class<?>[]{String.class,String.class,String.class,String.class},(Object[])values));systems.add(item);}output.add("system",systems);
  var users=new JsonArray();for(String[] values:new String[][]{{null,null},{" 统计人数 ","  SELECT id FROM users; "},{"","\u2003"},{"\u2003需求\u2003","\u00a0"}}){var item=new JsonObject();item.add("input",gson.toJsonTree(values));item.addProperty("output",(String)invoke("buildUserPrompt",new Class<?>[]{String.class,String.class},(Object[])values));users.add(item);}output.add("user",users);
  var content=new JsonArray();for(String body:List.of("{}","{\"choices\":[]}","{\"choices\":[{}]}","{\"choices\":[{\"message\":{}}]}","{\"choices\":[{\"message\":{\"content\":null}}]}","{\"choices\":[{\"message\":{\"content\":\"SELECT 1;\"}}]}","{\"choices\":[{\"message\":{\"content\":true}}]}","{\"choices\":[{\"message\":{\"content\":[{\"text\":\"SELECT \"},null,1,{\"text\":1},{\"other\":\"x\"}]}}]}")){
   var item=new JsonObject();item.addProperty("input",body);try{item.addProperty("output",(String)invoke("extractContent",one,body));}catch(InvocationTargetException error){item.addProperty("error",error.getCause().getMessage());}content.add(item);
  }output.add("content",content);
  var errors=new JsonArray();for(String body:Arrays.asList(null,"","\n ","not json\r\nline 2","{\"error\":{\"message\":\"接口拒绝\\n请检查模型\"}}","{\"error\":{\"message\":123}}","x".repeat(501),"😀".repeat(251))){var item=new JsonObject();item.addProperty("input",body);item.addProperty("output",(String)invoke("extractError",one,body));errors.add(item);}output.add("errors",errors);
  var path=Path.of(args[0]);Files.createDirectories(path.getParent());Files.writeString(path,gson.toJson(output));
 }
}
