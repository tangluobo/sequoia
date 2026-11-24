import com.google.gson.*;
import com.tangluobo.tomato.module.connect.TableRowData;
import com.tangluobo.tomato.module.connect.service.DatabaseService;
import java.nio.file.*;
import java.sql.*;
import java.net.*;
import java.util.*;
/** Calls the unchanged original service's metadata helper and its real JDBC drivers. */
public class SqlMetadataGolden {
 public static void main(String[] args)throws Exception {
  var output=new JsonObject();
  for(var kind:List.of("mysql","postgres"))output.add(kind,run(kind,System.getenv(kind.equals("mysql")?"TOMATO_TEST_MYSQL_URL":"TOMATO_TEST_POSTGRES_URL")));
  var path=Path.of(args[0]);Files.createDirectories(path.getParent());Files.writeString(path,new GsonBuilder().setPrettyPrinting().serializeNulls().disableHtmlEscaping().create().toJson(output));
 }
 static JsonArray run(String kind,String uriText)throws Exception {
  var uri=URI.create(uriText);if(!"127.0.0.1".equals(uri.getHost())||!uri.getPath().startsWith("/tomato_"))throw new IllegalArgumentException("isolated loopback database required");
  var credentials=uri.getRawUserInfo().split(":",2);var properties=new Properties();properties.setProperty("user",URLDecoder.decode(credentials[0],java.nio.charset.StandardCharsets.UTF_8));properties.setProperty("password",credentials.length>1?URLDecoder.decode(credentials[1],java.nio.charset.StandardCharsets.UTF_8):"");
  if(kind.equals("mysql")){properties.setProperty("sslMode","DISABLED");properties.setProperty("allowPublicKeyRetrieval","true");properties.setProperty("characterEncoding","UTF-8");properties.setProperty("tinyInt1isBit","false");}else properties.setProperty("sslmode","disable");
  var helper=DatabaseService.class.getDeclaredMethod("populateResultColumnMetadata",TableRowData.class,ResultSetMetaData.class,int.class);helper.setAccessible(true);
  var cases=new JsonArray();var gson=new Gson();
  try(var connection=DriverManager.getConnection("jdbc:"+(kind.equals("mysql")?"mysql":"postgresql")+"://127.0.0.1:"+uri.getPort()+uri.getPath(),properties);var statement=connection.createStatement()){
   statement.execute("DROP TABLE IF EXISTS tomato_origin_fixture");
   try{
    statement.execute("CREATE TABLE tomato_origin_fixture(id INTEGER PRIMARY KEY,label VARCHAR(30),stamp DATE,payload "+(kind.equals("mysql")?"BLOB":"BYTEA")+")");
    statement.execute("INSERT INTO tomato_origin_fixture(id,label,stamp) VALUES(1,'中文','2024-02-29'),(2,NULL,NULL)");
    for(var sql:List.of("SELECT id AS renamed_id,label,42 AS id FROM tomato_origin_fixture ORDER BY renamed_id","SELECT id,label,stamp,payload FROM tomato_origin_fixture WHERE 1=0","SELECT count(*) AS n FROM tomato_origin_fixture","SELECT 42 AS id,id AS actual_id,label FROM tomato_origin_fixture ORDER BY actual_id","SELECT a.id,b.label FROM tomato_origin_fixture a JOIN tomato_origin_fixture b ON a.id=b.id ORDER BY a.id")){
     var item=new JsonObject();item.addProperty("sql",sql);
     try(var rs=statement.executeQuery(sql)){
      var metadata=rs.getMetaData();var model=new TableRowData();helper.invoke(null,model,metadata,metadata.getColumnCount());
      item.add("labels",gson.toJsonTree(model.getColumnNames()));item.add("types",gson.toJsonTree(model.getColumnTypes()));item.add("columns",gson.toJsonTree(model.getSourceColumnNames()));item.add("tables",gson.toJsonTree(model.getSourceTableNames()));item.add("schemas",gson.toJsonTree(model.getSourceSchemaNames()));item.add("catalogs",gson.toJsonTree(model.getSourceCatalogNames()));
      var rows=new JsonArray();while(rs.next()){var row=new JsonArray();for(int i=1;i<=metadata.getColumnCount();i++)row.add(gson.toJsonTree(rs.getString(i)));rows.add(row);}item.add("rows",rows);
     }cases.add(item);
    }
   }finally{statement.execute("DROP TABLE IF EXISTS tomato_origin_fixture");}
  }
  return cases;
 }
}
