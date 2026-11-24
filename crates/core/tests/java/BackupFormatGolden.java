import com.google.gson.*;
import com.tangluobo.tomato.module.connect.service.BackupService;
import org.apache.commons.codec.digest.DigestUtils;
import org.apache.commons.compress.archivers.tar.*;
import java.nio.file.*;
import java.util.*;
import java.io.*;
import java.math.*;
/** Original backup path/literal/nb3 encoder only; no database, desktop or saved configuration. */
public class BackupFormatGolden {
 static java.lang.reflect.Method method(String name,Class<?>... types)throws Exception {var m=BackupService.class.getDeclaredMethod(name,types);m.setAccessible(true);return m;}
 static Gson gson=new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();
 static void entry(TarArchiveOutputStream tar,String name,byte[] bytes)throws Exception {var e=new TarArchiveEntry(name);e.setSize(bytes.length);tar.putArchiveEntry(e);tar.write(bytes);tar.closeArchiveEntry();}
 public static void main(String[] args)throws Exception {
  Path output=Path.of(args[0]);Files.createDirectories(output);
  var result=new JsonObject();var paths=new JsonArray();
  var sanitizer=method("sanitizeFileName",String.class);var resolver=method("resolveBackupFile",Path.class,String.class);
  for(String name:Arrays.asList(null,""," ",".","..","___","a/b:c*?\"<>|","中文 连接","a\t\rb","\u00a0","\u2007","\u202f","\u001c","\u2003"," leading ","...")) {
   var row=new JsonObject();if(name==null)row.add("input",JsonNull.INSTANCE);else row.addProperty("input",name);
   row.addProperty("sanitized",(String)sanitizer.invoke(null,name));
   Path root=output.resolve("files").toAbsolutePath();try{row.addProperty("filename",root.relativize((Path)resolver.invoke(null,root,name)).toString());}catch(java.lang.reflect.InvocationTargetException error){row.addProperty("filenameError",error.getCause().getClass().getSimpleName());}paths.add(row);
  }
  result.add("paths",paths);
  var literals=new JsonArray();
  Object[][] values={{"null",null},{"empty",""},{"quote","O'Reilly"},{"backslash","C:\\tmp\\one"},{"unicode","中文😀"},{"separator","one\u001e\ntwo"},{"bool",true},{"negative",-17},{"decimal",new BigDecimal("12345678901234567890.0012300")},{"date",java.sql.Date.valueOf("2024-02-29")},{"time",java.sql.Time.valueOf("12:34:56")},{"timestamp",java.sql.Timestamp.valueOf("2024-02-29 12:34:56.123456789")},{"localDate",java.time.LocalDate.of(2024,2,29)},{"localTime",java.time.LocalTime.of(12,34,56,123000000)},{"localDateTime",java.time.LocalDateTime.of(2024,2,29,12,34,56,123000000)},{"map",new LinkedHashMap<>(Map.of("value","<中文>\u2028"))},{"list",List.of(1,"'",false)}};
  var literal=method("toSqlLiteral",Object.class);
  for(var value:values){var row=new JsonObject();row.addProperty("kind",(String)value[0]);row.addProperty("sql",(String)literal.invoke(null,value[1]));literals.add(row);}
  result.add("literals",literals);
  String uuid="01234567-89AB-CDEF-0123-456789ABCDEF";
  List<Map<String,Object>> records=new ArrayList<>();
  for(var pair:List.of(new Object[]{1L,"中文 O'Reilly"},new Object[]{2L,"one\u001e\ntwo"},new Object[]{3L,"line\nnext"})){var row=new LinkedHashMap<String,Object>();row.put("id",pair[0]);row.put("value",pair[1]);records.add(row);}
  byte[] data=(byte[])method("toGzipBytes",List.class).invoke(null,records);
  String dataName=uuid+".data.00000.sql.gz";
  var definition=new BackupService.Nb3TableMeta();definition.setMetaVersion("30100");definition.setName("legacy_sample");definition.setType("Table");definition.setDdl("CREATE TABLE legacy_sample(id BIGINT NOT NULL, value TEXT)");definition.setSubddl(new String[]{});definition.setAutoIncrement("");definition.setFields(new String[]{"id","value"});definition.setTriggerDDL(new String[]{});definition.setIndexDDL(new String[]{});definition.setData(new BackupService.Nb3TableData[]{new BackupService.Nb3TableData(dataName,DigestUtils.sha1Hex(data))});
  byte[] def=(byte[])method("gzipBytes",String.class).invoke(null,gson.toJson(definition));String defName=uuid+".meta.json.gz";
  var object=new BackupService.Nb3Object();object.setName("legacy_sample");object.setType("Table");object.setUuid(uuid);object.setRows("3");var pointer=new BackupService.Nb3MetaData();pointer.setFilename(defName);pointer.setChecksum(DigestUtils.sha1Hex(def));object.setMetadata(pointer);
  var metadata=new BackupService.Nb3Meta();metadata.setMetaVersion("30100");metadata.setDatabaseType("MYSQL");metadata.setCatalog("");metadata.setSchema("legacy_source");metadata.setStartTime("1");metadata.setEndTime("2");metadata.setEncryption("None");metadata.setComment("旧备份格式");metadata.setObjects(List.of(object));
  try(var tar=new TarArchiveOutputStream(Files.newOutputStream(output.resolve("original.nb3")))){tar.setLongFileMode(TarArchiveOutputStream.LONGFILE_POSIX);entry(tar,dataName,data);entry(tar,defName,def);entry(tar,"meta.json",gson.toJson(metadata).getBytes(java.nio.charset.StandardCharsets.UTF_8));}
  result.add("metadata",gson.toJsonTree(metadata));result.add("definition",gson.toJsonTree(definition));
  Files.writeString(output.resolve("original-format.json"),gson.toJson(result));
 }
}
