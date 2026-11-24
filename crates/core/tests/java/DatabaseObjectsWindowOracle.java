import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.*;
import com.tangluobo.tomato.module.connect.service.DatabaseService;
import com.tangluobo.tomato.module.connect.view.TableObjectsView;
import java.nio.file.*;
import java.lang.reflect.*;
import java.sql.*;
import java.util.*;
import javax.imageio.ImageIO;

/** Original view and real loader code, with an in-process synthetic JDBC adapter.
 * No socket, saved configuration, system clipboard or operating-system input. */
public class DatabaseObjectsWindowOracle {
 static Object field(Object value,String name)throws Exception {var f=TableObjectsView.class.getDeclaredField(name);f.setAccessible(true);return f.get(value);}
 static void call(Object value,String name)throws Exception {var method=TableObjectsView.class.getDeclaredMethod(name);method.setAccessible(true);method.invoke(value);}
 static Map<String,Object> row(Object... pairs){var row=new LinkedHashMap<String,Object>();for(int i=0;i<pairs.length;i+=2)row.put((String)pairs[i],pairs[i+1]);return row;}
 static List<Map<String,Object>> rows(String sql){
  if(sql.contains("FROM information_schema.TABLES"))return List.of(
   row("TABLE_NAME","orders","TABLE_TYPE","BASE TABLE","ENGINE","InnoDB","AUTO_INCREMENT","3","UPDATE_TIME","2024-02-29 12:34:56","DATA_LENGTH",16384L,"TABLE_ROWS",2L,"TABLE_COMMENT","订单"),
   row("TABLE_NAME","users","TABLE_TYPE","BASE TABLE","ENGINE","InnoDB","AUTO_INCREMENT","2","UPDATE_TIME",null,"DATA_LENGTH",16384L,"TABLE_ROWS",1L,"TABLE_COMMENT","用户"),
   row("TABLE_NAME","active_users","TABLE_TYPE","VIEW","ENGINE",null,"AUTO_INCREMENT",null,"UPDATE_TIME",null,"DATA_LENGTH",0L,"TABLE_ROWS",null,"TABLE_COMMENT","用户视图"));
  if(sql.contains("KEY_COLUMN_USAGE")&&sql.contains("GROUP_CONCAT"))return List.of(row("TABLE_NAME","orders","CONSTRAINT_NAME","fk_user","COLUMNS","user_id","REF_DB","sample_db","REF_TABLE","users","REF_COLUMNS","id","DELETE_RULE","RESTRICT","UPDATE_RULE","CASCADE"));
  return List.of();
 }
 static Object basic(Method method){
  if(method.getReturnType()==boolean.class)return false;
  if(method.getReturnType()==int.class)return 0;
  if(method.getReturnType()==long.class)return 0L;
  return null;
 }
 @SuppressWarnings("unchecked")
 static <T>T proxy(Class<T> type,InvocationHandler handler){return (T)Proxy.newProxyInstance(DatabaseObjectsWindowOracle.class.getClassLoader(),new Class<?>[]{type},handler);}
 static ResultSet result(List<Map<String,Object>> rows){
  int[] cursor={-1};boolean[] wasNull={false};
  return proxy(ResultSet.class,(self,method,args)->{
   String name=method.getName();
   if(name.equals("next"))return ++cursor[0]<rows.size();
   if(name.equals("wasNull"))return wasNull[0];
   if(name.equals("getString")||name.equals("getLong")||name.equals("getInt")){
    Object value=rows.get(cursor[0]).get(String.valueOf(args[0]));wasNull[0]=value==null;
    if(name.equals("getString"))return value==null?null:value.toString();
    long number=value==null?0L:Long.parseLong(value.toString());
    if(name.equals("getInt"))return (int)number;
    return number;
   }
   return basic(method);
  });
 }
 static PreparedStatement statement(String sql){return proxy(PreparedStatement.class,(self,method,args)->{
  if(method.getName().equals("executeQuery"))return result(rows(args!=null&&args.length>0?(String)args[0]:sql));
  return basic(method);
 });}
 static java.sql.Connection connection(){return proxy(java.sql.Connection.class,(self,method,args)->switch(method.getName()){
  case "isValid"->true;
  case "prepareStatement"->statement((String)args[0]);
  case "createStatement"->statement("");
  case "getMetaData"->proxy(DatabaseMetaData.class,(p,m,a)->{
   if(m.getName().equals("getPrimaryKeys")||m.getName().equals("getColumns"))return result(List.of());
   if(m.getName().equals("getDatabaseProductVersion"))return "8.4.11";
   return basic(m);
  });
  default->basic(method);
 });}
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  @SuppressWarnings({"unchecked","rawtypes"})
  public void start(Stage stage)throws Exception {
   var config=new ConnectionConfig();config.setType(ConnectType.MYSQL);config.setName("本地数据库");config.setId("objects-oracle-only");
   var cache=DatabaseService.class.getDeclaredField("connectionCache");cache.setAccessible(true);((Map)cache.get(null)).put(config.getId(),connection());
   var operations=proxy(TableObjectsView.ObjectOperations.class,(self,method,args)->null);
   var view=new TableObjectsView(config,"sample_db",null,operations);
   stage.setScene(new Scene(view,896,650));stage.setOpacity(.01);stage.show();
   var wait=new PauseTransition(Duration.seconds(2));wait.setOnFinished(event->{try{
    if(!((Label)field(view,"countLabel")).getText().startsWith("共 3 个对象"))throw new AssertionError("Original loader did not consume synthetic JDBC data");
    if(getParameters().getRaw().get(1).equals("SELECTION")){
     selection(view,Path.of(getParameters().getRaw().get(0)));stage.close();Platform.exit();System.exit(0);return;
    }
    Class type=Arrays.stream(TableObjectsView.class.getDeclaredClasses()).filter(c->c.getSimpleName().equals("ViewType")).findFirst().orElseThrow();
    var switcher=TableObjectsView.class.getDeclaredMethod("switchView",type);switcher.setAccessible(true);switcher.invoke(view,Enum.valueOf(type,getParameters().getRaw().get(1)));
    if(getParameters().getRaw().size()>2)call(view,"selectAll");
    view.applyCss();view.layout();
    var capture=new PauseTransition(Duration.millis(300));capture.setOnFinished(done->{try{
     var path=Path.of(getParameters().getRaw().get(0));
     ImageIO.write(SwingFXUtils.fromFXImage(view.snapshot(null,null),null),"png",path.toFile());
     var out=new JsonObject();out.addProperty("width",stage.getScene().getWidth());out.addProperty("height",stage.getScene().getHeight());out.addProperty("syntheticJdbcOnly",true);out.addProperty("socketsOpened",false);out.addProperty("systemClipboardWritten",false);out.addProperty("status",((Label)field(view,"countLabel")).getText());var controls=new JsonArray();walk(view,controls);out.add("controls",controls);
     Files.writeString(Path.of(path+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(out));stage.close();Platform.exit();System.exit(0);
    }catch(Throwable error){error.printStackTrace();System.exit(1);}});capture.play();
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
  }
 }
 @SuppressWarnings("unchecked")
 static void selection(TableObjectsView view,Path output)throws Exception {
  List<?> original=new ArrayList<>((List<?>)field(view,"allObjects"));
  Class<?> type=original.get(0).getClass();var name=type.getDeclaredField("name");name.setAccessible(true);
  var add=TableObjectsView.class.getDeclaredMethod("addToSelection",type);add.setAccessible(true);
  var toggle=TableObjectsView.class.getDeclaredMethod("toggleSelection",type);toggle.setAccessible(true);
  var range=TableObjectsView.class.getDeclaredMethod("selectRange",type,type);range.setAccessible(true);
  var search=TableObjectsView.class.getDeclaredMethod("applySearch",String.class);search.setAccessible(true);
  var sequences=List.of(
   List.of("click:2","ctrl:0","ctrl:2"),List.of("click:2","ctrl:0","shift:1"),
   List.of("click:2","ctrl:0","ctrlshift:1"),List.of("click:1","all","shift:2"),
   List.of("click:2","ctrl:0","search:用户"),List.of("click:1","search:订单","search:","shift:2"),
   List.of("all","ctrl:1","ctrl:0"),List.of("click:0","search:\u2003订单\u2003"));
  var result=new JsonArray();
  for(var sequence:sequences){
   call(view,"clearSelection");((TextField)field(view,"searchField")).setText("");search.invoke(view,"");var test=new JsonObject();var actions=new JsonArray();var snapshots=new JsonArray();
   for(String action:sequence){
    actions.add(action);
    if(action.equals("all"))call(view,"selectAll");
    else if(action.startsWith("search:")){String text=action.substring(7);((TextField)field(view,"searchField")).setText(text);search.invoke(view,text);}
    else {String[] parts=action.split(":");var object=original.get(Integer.parseInt(parts[1]));
     if(parts[0].equals("click")){call(view,"clearSelection");add.invoke(view,object);}
     else if(parts[0].equals("ctrl"))toggle.invoke(view,object);
     else {var anchor=field(view,"anchorObject");if(anchor==null){call(view,"clearSelection");add.invoke(view,object);}else range.invoke(view,anchor,object);}
    }
    var state=new JsonObject();var selected=new JsonArray();for(Object object:(Set<?>)field(view,"selectedObjects"))selected.add((String)name.get(object));state.add("selected",selected);
    var visible=new JsonArray();for(Object object:(List<?>)field(view,"objects"))visible.add((String)name.get(object));state.add("visible",visible);
    for(String key:List.of("selectedObject","anchorObject")){var object=field(view,key);if(object==null)state.add(key,JsonNull.INSTANCE);else state.addProperty(key,(String)name.get(object));}
    state.addProperty("status",((Label)field(view,"countLabel")).getText());snapshots.add(state);
   }
   test.add("actions",actions);test.add("states",snapshots);result.add(test);
  }
  Files.writeString(output,new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(result));
 }
 static void walk(Node node,JsonArray out){
  if(!node.isVisible())return;
  if(node.isVisible()&&node instanceof Control control){
   var bounds=node.localToScene(node.getLayoutBounds());var row=new JsonObject();row.addProperty("class",node.getClass().getSimpleName());
   if(control instanceof Labeled label){row.addProperty("text",label.getText());row.addProperty("fontName",label.getFont().getName());row.addProperty("fontSize",label.getFont().getSize());var nativeFont=com.sun.javafx.scene.text.FontHelper.getNativeFont(label.getFont());if(nativeFont instanceof com.sun.javafx.font.PGFont font){row.addProperty("fontFile",font.getFontResource().getFileName());}}
   row.addProperty("x",bounds.getMinX());row.addProperty("y",bounds.getMinY());row.addProperty("width",bounds.getWidth());row.addProperty("height",bounds.getHeight());row.addProperty("disabled",control.isDisabled());out.add(row);
  }
  if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())walk(child,out);
 }
}
