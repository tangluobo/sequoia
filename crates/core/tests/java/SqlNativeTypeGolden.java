import java.nio.file.*;
import java.sql.*;
import java.net.*;
import java.util.*;
import com.google.gson.*;
/** Same JDBC versions and ResultSet.getString used by the original DatabaseService. */
public class SqlNativeTypeGolden {
 public static void main(String[] args)throws Exception {
  var out=Path.of(args[0]);Files.createDirectories(out);
  run("mysql",System.getenv("TOMATO_TEST_MYSQL_URL"),out.resolve("mysql.json"));
  run("postgres",System.getenv("TOMATO_TEST_POSTGRES_URL"),out.resolve("postgres.json"));
 }
 static void run(String kind,String uriText,Path output)throws Exception {
  var uri=URI.create(uriText);
  if(!uri.getHost().equals("127.0.0.1")||!uri.getPath().startsWith("/tomato_"))throw new IllegalArgumentException("dedicated loopback fixture required");
  var info=uri.getRawUserInfo().split(":",2);var properties=new Properties();properties.setProperty("user",URLDecoder.decode(info[0],java.nio.charset.StandardCharsets.UTF_8));properties.setProperty("password",info.length==2?URLDecoder.decode(info[1],java.nio.charset.StandardCharsets.UTF_8):"");
  if(kind.equals("mysql")){properties.setProperty("sslMode","DISABLED");properties.setProperty("allowPublicKeyRetrieval","true");properties.setProperty("characterEncoding","UTF-8");}else{properties.setProperty("sslmode","disable");}
  String jdbc="jdbc:"+(kind.equals("mysql")?"mysql":"postgresql")+"://127.0.0.1:"+uri.getPort()+uri.getPath();
  String[] setup=kind.equals("mysql")?new String[]{"SET time_zone='+00:00'","CREATE TEMPORARY TABLE tomato_type_fixture(b BIT(8),payload BLOB)","INSERT INTO tomato_type_fixture VALUES(b'10100101',UNHEX('00ff41'))"}:new String[]{"SET TIME ZONE 'UTC'"};
  String[] sql=kind.equals("mysql")?new String[]{
   "SELECT CAST('2024-02-29' AS DATE) AS d, CAST('12:34:56.123000' AS TIME(6)) AS t, CAST('2024-02-29 12:34:56.123000' AS DATETIME(6)) AS ts",
   "SELECT CAST('12345678901234567890.1234567890' AS DECIMAL(30,10)) AS n, CAST('18446744073709551615' AS UNSIGNED) AS u",
   "SELECT CAST('{\"中文\":[1,null]}' AS JSON) AS j, NULL AS empty_value, 'NULL' AS literal_value",
   "SELECT b,payload FROM tomato_type_fixture",
   "SELECT 0.0000000001 AS small_value, 1e100 AS large_value, true AS yes_value, false AS no_value"
  }:new String[]{
   "SELECT DATE '2024-02-29' AS d, TIME '12:34:56.123000' AS t, TIMESTAMP '2024-02-29 12:34:56.123000' AS ts, TIMESTAMPTZ '2024-02-29 12:34:56+08' AS tz",
   "SELECT NUMERIC '123456789012345678901234567890.01234567890123456789' AS n, 'Infinity'::float8 AS infinite_value, 'NaN'::float8 AS nan_value",
   "SELECT '{\"中文\":[1,null]}'::json AS j, '{\"中文\":[1,null]}'::jsonb AS jb, NULL AS empty_value, 'NULL'::text AS literal_value",
   "SELECT ARRAY[1,NULL,3] AS a, INTERVAL '3 days 04:05:06.1234' AS duration, UUID '123e4567-e89b-12d3-a456-426614174000' AS identifier",
   "SELECT decode('00ff41','hex') AS payload, B'10100101' AS bits, true AS yes_value, false AS no_value"
  };
  var result=new JsonObject();result.add("setup",new Gson().toJsonTree(setup));var cases=new JsonArray();
  try(var connection=DriverManager.getConnection(jdbc,properties);var statement=connection.createStatement()){
   for(var command:setup)statement.execute(command);
   for(var command:sql){var item=new JsonObject();item.addProperty("sql",command);var columns=new JsonArray();var rows=new JsonArray();
    try(var rs=statement.executeQuery(command)){var metadata=rs.getMetaData();for(int i=1;i<=metadata.getColumnCount();i++)columns.add(metadata.getColumnLabel(i));
     while(rs.next()){var row=new JsonArray();for(int i=1;i<=metadata.getColumnCount();i++){var value=rs.getString(i);row.add(value==null?JsonNull.INSTANCE:new JsonPrimitive(value));}rows.add(row);}
    }item.add("columns",columns);item.add("rows",rows);cases.add(item);
   }
  }
  result.add("cases",cases);Files.writeString(output,new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(result));
 }
}
