import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.control.*;
import javafx.util.Duration;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.view.TableStructureView;
import java.nio.file.*;
import java.util.*;

/** Calls only owned original draft actions. No clipboard, SQL writes or OS input. */
public class TableDesignStateOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends TableDesignWindowOracle.Probe {
  JsonArray output=new JsonArray();int sequence,index;JsonObject current;
  List<List<String>> sequences=List.of(
   List.of("select:1","pk","up","up","down","delete"),
   List.of("select:2","add","insert","up","down","delete","all","delete","add"),
   List.of("up","down","pk","delete","insert"),
   List.of("select:1","paste:[{\"字段名\":\"amount\",\"注释\":\"含引号'\\\"<&>\\n文字\"},{\"字段名\":\"enabled\",\"类型\":\"BOOLEAN\",\"非空\":\"是\",\"主键\":\"否\",\"自增\":null}]","pk","up","delete"),
   List.of("all","delete","paste:[{\"字段名\":\"user_id\",\"长度\":\"42\",\"小数点\":\"3\",\"未知键\":\"忽略\"}]"),
   List.of("paste:[]","paste:null"),
   List.of("paste:[{\"字段名\":\"flags\",\"非空\":\"true\",\"主键\":false,\"自增\":\"maybe\",\"默认值\":1.00,\"注释\":true}]","pk","up","down","delete"));
  void call(String name)throws Exception {var method=TableStructureView.class.getDeclaredMethod(name);method.setAccessible(true);method.invoke(view);}
  @SuppressWarnings({"unchecked","rawtypes"})
  @Override void capture(int ignored)throws Exception {
   var titles=(List<String>)TableDesignWindowOracle.field(view,"columnTitles");
   var rows=new ArrayList<Map<String,String>>();
   for(var name:List.of("first","second","third")){
    var row=new LinkedHashMap<String,String>();for(var title:titles)row.put(title,"");
    row.put("字段名",name);row.put("类型","INT");
    for(var title:List.of("非空","主键","自增","无符号","填充零","二进制"))row.put(title,"否");rows.add(row);
   }
   var method=TableStructureView.class.getDeclaredMethod("updateTableView",List.class);method.setAccessible(true);method.invoke(view,rows);call("clearDirty");
   var table=(TableView)TableDesignWindowOracle.field(view,"tableView");table.getSelectionModel().clearSelection();table.getFocusModel().focus(-1);
   current=new JsonObject();current.addProperty("sequence",sequence);current.add("steps",new JsonArray());output.add(current);index=0;
   snapshot("initial");next();
  }
  @SuppressWarnings({"unchecked","rawtypes"})
  void next()throws Exception {
   if(index==sequences.get(sequence).size()){
    sequence++;if(sequence<sequences.size()){capture(0);return;}
    Files.writeString(Path.of(getParameters().getRaw().get(0)),new GsonBuilder().setPrettyPrinting().create().toJson(output));stage.close();Platform.exit();System.exit(0);return;
   }
   var operation=sequences.get(sequence).get(index++);
   var table=(TableView)TableDesignWindowOracle.field(view,"tableView");table.edit(-1,null);
   if(operation.startsWith("select:")){
    int row=Integer.parseInt(operation.substring(7));table.getSelectionModel().clearSelection();table.getSelectionModel().select(row,(TableColumn)table.getColumns().get(1));
   }else if(operation.startsWith("paste:")){
    var cached=TableStructureView.class.getDeclaredField("copiedFieldsJson");cached.setAccessible(true);cached.set(view,operation.substring(6));call("handlePasteFields");
   }else if(operation.equals("all"))table.getSelectionModel().selectAll();
   else call(switch(operation){case "add"->"handleAddField";case "insert"->"handleInsertField";case "pk"->"handleTogglePrimaryKey";case "up"->"handleMoveUp";case "down"->"handleMoveDown";default->"handleDeleteField";});
   var pause=new PauseTransition(Duration.millis(100));pause.setOnFinished(event->{try{snapshot(operation);next();}catch(Throwable error){fail(error);}});pause.play();
  }
  @SuppressWarnings({"unchecked","rawtypes"})
  void snapshot(String operation)throws Exception {
   var table=(TableView)TableDesignWindowOracle.field(view,"tableView");
   var result=new JsonObject();result.addProperty("operation",operation);result.add("rows",new Gson().toJsonTree(table.getItems()));
   var cells=new JsonArray();
   for(var value:table.getSelectionModel().getSelectedCells()){
    var cell=(TablePosition)value;if(cell.getColumn()<=0)continue;
    var point=new JsonArray();point.add(cell.getRow());point.add(cell.getColumn()-1);cells.add(point);
   }
   result.add("selected",cells);var focus=table.getFocusModel().getFocusedCell();
   // Pure source-format projection; never call handleCopyFields (it writes the OS clipboard).
   var indices=new ArrayList<Integer>((Collection<Integer>)table.getSelectionModel().getSelectedIndices());
   if(indices.isEmpty()&&focus.getRow()>=0)indices.add(focus.getRow());Collections.sort(indices);
   var copied=new ArrayList<Map<String,String>>();var titles=(List<String>)TableDesignWindowOracle.field(view,"columnTitles");
   for(int index:indices){var row=(List<String>)table.getItems().get(index);var map=new LinkedHashMap<String,String>();for(int c=0;c<titles.size();c++)map.put(titles.get(c),row.get(c)==null?"":row.get(c));copied.add(map);}
   result.add("copyJson",copied.isEmpty()?JsonNull.INSTANCE:new JsonPrimitive(new GsonBuilder().create().toJson(copied)));
   result.addProperty("focusRow",focus.getRow());result.addProperty("focusColumn",Math.max(0,focus.getColumn()-1));
   result.addProperty("dirty",(Boolean)TableDesignWindowOracle.field(view,"dirty"));result.addProperty("status",((TextInputControl)TableDesignWindowOracle.field(view,"statusLabel")).getText());
   current.getAsJsonArray("steps").add(result);
  }
 }
}
