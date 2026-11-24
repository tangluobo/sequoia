package com.tangluobo.tomato.module.connect.view;
import java.nio.file.*;
import com.google.gson.*;
public class SqlQueryPathGolden {
 public static void main(String[] args)throws Exception {
  var method=SqlEditorView.class.getDeclaredMethod("sanitizeFileName",String.class);method.setAccessible(true);
  JsonObject output=new JsonObject();JsonArray names=new JsonArray();
  for(String text:new String[]{null,"","mysql:生产 / A","  a\t\r\nb  ","__a___b__","???"," ","中文🍅","a\u00a0b","a\u2003b","a\u000bb","CON","name.sql","._a_."}) {
   JsonObject row=new JsonObject();row.add("input",text==null?JsonNull.INSTANCE:new JsonPrimitive(text));row.addProperty("result",(String)method.invoke(null,text));names.add(row);
  }
  output.add("names",names);JsonArray paths=new JsonArray();Path root=Path.of(System.getProperty("user.home"),".tomato");
  for(String path:new String[]{null,"","child","first/second","/child","child/","a//b","/","a\\b"}) {
   JsonObject row=new JsonObject();row.add("path",path==null?JsonNull.INSTANCE:new JsonPrimitive(path));row.addProperty("relative",root.relativize(SqlEditorView.resolveQueryDir("生产 A","my db",path)).toString().replace('\\','/'));paths.add(row);
  }
  output.add("paths",paths);Files.writeString(Path.of(args[0]),new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(output));
 }
}
