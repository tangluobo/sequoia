import javafx.application.*;
import javafx.scene.control.*;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.dialog.BackupDialog;
import com.tangluobo.tomato.module.connect.ConnectionConfig;
import com.tangluobo.tomato.module.connect.ConnectType;
import java.nio.file.*;
import java.util.*;
/** Exercises original listeners directly; never shows a window or starts a loader. */
public class BackupSelectionGolden {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  @SuppressWarnings("unchecked")
  public void start(javafx.stage.Stage stage)throws Exception {
   var config=new ConnectionConfig();config.setType(ConnectType.MYSQL);config.setName("fixture");
   var dialog=new BackupDialog(null,config,"sample_db","");
   var build=BackupDialog.class.getDeclaredMethod("buildObjectTree",List.class,List.class,List.class,List.class);build.setAccessible(true);
   var field=BackupDialog.class.getDeclaredField("objectTree");field.setAccessible(true);
   var all=BackupDialog.class.getDeclaredMethod("selectAllObjects",boolean.class);all.setAccessible(true);
   var selected=BackupDialog.class.getDeclaredMethod("getSelectedBackupObjects");selected.setAccessible(true);
   var result=new JsonArray();
   var cases=List.of(List.of("child0+","all-"),List.of("custom+","child0-","child1-"),List.of("all+","child0+","child1+"),List.of("child0+","child1+","custom-"),List.of("all+","all-"),List.of("custom+","all+","all-"),List.of("child0+","custom-"));
   for(var ops:cases){
    build.invoke(dialog,List.of(new BackupDialog.BackupObject(BackupDialog.BackupObject.Type.TABLE,"users"),new BackupDialog.BackupObject(BackupDialog.BackupObject.Type.TABLE,"orders")),List.of(),List.of(),List.of());
    var tree=(TreeView<BackupDialog.BackupObject>)field.get(dialog);var group=tree.getRoot().getChildren().get(0);var runtime=group.getChildren().get(0).getValue();var custom=group.getChildren().get(1);
    var trace=new JsonArray();
    for(var op:ops){
     boolean value=op.endsWith("+");
     if(op.startsWith("all"))all.invoke(dialog,value);else if(op.startsWith("custom"))custom.getValue().setSelected(value);else custom.getChildren().get(op.charAt(5)-'0').getValue().setSelected(value);
     var state=new JsonObject();state.addProperty("all",runtime.isSelected());state.addProperty("custom",custom.getValue().isSelected());state.addProperty("indeterminate",custom.getValue().isIndeterminate());
     var flags=new JsonArray();for(var child:custom.getChildren())flags.add(child.getValue().isSelected());state.add("children",flags);
     var names=new JsonArray();for(var item:(List<BackupDialog.BackupObject>)selected.invoke(dialog))names.add(item.getDisplayName());state.add("selected",names);trace.add(state);
    }
    var item=new JsonObject();item.add("operations",new Gson().toJsonTree(ops));item.add("trace",trace);result.add(item);
   }
   Files.writeString(Path.of(getParameters().getRaw().get(0)),new GsonBuilder().setPrettyPrinting().create().toJson(result));
   Platform.exit();System.exit(0);
  }
 }
}
