import javafx.application.*;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.view.SqlEditorView;
import java.nio.file.*;
import java.lang.reflect.*;
import java.util.*;
/** Actual directive, insertion and schema-ranking methods. No HTTP/DB/clipboard. */
public class AiSqlEditorGolden {
 public static void main(String[] args){Platform.startup(()->{try{run(Path.of(args[0]));Platform.exit();System.exit(0);}catch(Throwable error){error.printStackTrace();System.exit(1);}});}
 static void run(Path path)throws Exception {
  var view=new SqlEditorView(List.of(),null,null);var gson=new GsonBuilder().serializeNulls().setPrettyPrinting().disableHtmlEscaping().create();var out=new JsonObject();
  var directive=SqlEditorView.class.getDeclaredMethod("findAiDirective",String.class,int.class);directive.setAccessible(true);var after=SqlEditorView.class.getDeclaredMethod("createAiInsertionAfter",String.class,int.class);after.setAccessible(true);
  var directives=new JsonArray();var insertions=new JsonArray();
  for(String text:List.of("","SELECT 1;","-- ai: 统计人数","  -- AI： users ","--ai: ","--ai:","--ai:\t","-- ai: 前缀😀后缀\r\nSELECT 1;","SELECT 1;\n-- ai: orders\n尾行","\n-- ai: 第二行","-- Aİ: 不匹配","-- ai: \u2003","-- ai: a\rb","-- ai: a\u2028b","-- ai: a\r\r\n","a\rb\n尾行")){
   int count=text.codePointCount(0,text.length());for(int caret=0;caret<=count;caret++){int position=text.offsetByCodePoints(0,caret);var item=new JsonObject();item.addProperty("text",text);item.addProperty("caret",caret);try{Object value=directive.invoke(view,text,position);item.add("output",gson.toJsonTree(value));if(value!=null){var end=value.getClass().getDeclaredMethod("lineEnd");end.setAccessible(true);item.addProperty("lineEndCodepoints",text.codePointCount(0,(Integer)end.invoke(value)));}}catch(InvocationTargetException error){item.addProperty("error",error.getCause().getClass().getSimpleName());}directives.add(item);
    var insertion=new JsonObject();insertion.addProperty("text",text);insertion.addProperty("caret",caret);Object value=after.invoke(view,text,position);insertion.add("output",gson.toJsonTree(value));insertions.add(insertion);
   }
  }out.add("directives",directives);out.add("insertions",insertions);
  var schemaType=Class.forName(SqlEditorView.class.getName()+"$AiSchemaTable");var ctor=schemaType.getDeclaredConstructor(String.class,List.class,int.class);ctor.setAccessible(true);var field=SqlEditorView.class.getDeclaredField("aiSchemaTables");field.setAccessible(true);var context=SqlEditorView.class.getDeclaredMethod("buildAiSchemaContext",String.class);context.setAccessible(true);
  var contexts=new JsonArray();var cases=List.of(
   List.of(new String[]{"TABLE users\n  id BIGINT PRIMARY KEY\n  email VARCHAR -- 邮箱","users|id|email|邮箱"},new String[]{"TABLE orders\n  user_id BIGINT","orders|user_id"},new String[]{"TABLE a\n  id INT","a|id"}),
   List.of(new String[]{"x".repeat(48001),"x"},new String[]{"TABLE small\n  id INT","small"}),
   java.util.stream.IntStream.range(0,81).mapToObj(i->new String[]{"TABLE t"+i+"\n  id INT","t"+i}).toList());
  for(int dataset=0;dataset<cases.size();dataset++)for(String prompt:List.of("","users email","用户邮箱","user-id","`orders`.id","t80")){
   var tables=new ArrayList<Object>();var input=new JsonArray();for(int i=0;i<cases.get(dataset).size();i++){var pair=cases.get(dataset).get(i);var terms=List.of(pair[1].split("\\|"));tables.add(ctor.newInstance(pair[0],terms,i));var table=new JsonObject();table.addProperty("text",pair[0]);table.add("terms",gson.toJsonTree(terms));table.addProperty("order",i);input.add(table);}field.set(view,tables);var item=new JsonObject();item.addProperty("dataset",dataset);item.addProperty("prompt",prompt);item.add("tables",input);item.addProperty("output",(String)context.invoke(view,prompt));contexts.add(item);
  }out.add("contexts",contexts);
  var insertionType=Class.forName(SqlEditorView.class.getName()+"$AiInsertion");var apply=SqlEditorView.class.getDeclaredMethod("insertGeneratedSql",insertionType,String.class);apply.setAccessible(true);var editorField=SqlEditorView.class.getDeclaredField("editor");editorField.setAccessible(true);Object editor=editorField.get(view);var select=editor.getClass().getDeclaredMethod("selectRange",int.class,int.class);select.setAccessible(true);var start=editor.getClass().getDeclaredMethod("getSelectionStart");start.setAccessible(true);var end=editor.getClass().getDeclaredMethod("getSelectionEnd");end.setAccessible(true);
  var edits=new JsonArray();for(String[] values:new String[][]{{"SELECT 1;","SELECT 1;","SELECT 2;"},{"-- ai: 原说明\n","-- ai: 原说明\n","SELECT '😀';"},{"旧内容","修改后的😀正文","SELECT 3;"},{"abc\ndef","abc\ndef","SELECT 4;"}}){
   for(int caret:new int[]{0,values[1].codePointCount(0,values[1].length())}){Object insertion=after.invoke(view,values[0],values[0].length());view.setSqlText(values[1]);int position=values[1].offsetByCodePoints(0,caret);select.invoke(editor,position,position);apply.invoke(view,insertion,values[2]);String text=view.getSqlText();var item=new JsonObject();item.addProperty("snapshot",values[0]);item.addProperty("current",values[1]);item.addProperty("caret",caret);item.addProperty("sql",values[2]);item.addProperty("output",text);item.addProperty("selectionStart",text.codePointCount(0,(Integer)start.invoke(editor)));item.addProperty("selectionEnd",text.codePointCount(0,(Integer)end.invoke(editor)));edits.add(item);}
  }out.add("edits",edits);Files.createDirectories(path.getParent());Files.writeString(path,gson.toJson(out));
 }
}
