import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.dialog.BackupDialog;
import com.tangluobo.tomato.module.connect.ConnectionConfig;
import com.tangluobo.tomato.module.connect.ConnectType;
import java.nio.file.*;
import javax.imageio.ImageIO;
import java.util.*;
/** Original ready-state backup wizard, with synthetic objects; loader disabled before showing. */
public class BackupDialogOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 static Object field(Object value,String name)throws Exception {var f=BackupDialog.class.getDeclaredField(name);f.setAccessible(true);return f.get(value);}
 public static class Probe extends Application {
  public void start(Stage owner)throws Exception {
   var config=new ConnectionConfig();config.setType(ConnectType.MYSQL);config.setName("本地数据库");
   var dialog=new BackupDialog(null,config,"sample_db","");
   var loaded=BackupDialog.class.getDeclaredField("objectLoadStarted");loaded.setAccessible(true);loaded.setBoolean(dialog,true);
   var build=BackupDialog.class.getDeclaredMethod("buildObjectTree",List.class,List.class,List.class,List.class);build.setAccessible(true);
   build.invoke(dialog,List.of(new BackupDialog.BackupObject(BackupDialog.BackupObject.Type.TABLE,"users"),new BackupDialog.BackupObject(BackupDialog.BackupObject.Type.TABLE,"orders")),List.of(new BackupDialog.BackupObject(BackupDialog.BackupObject.Type.VIEW,"active_users")),List.of(new BackupDialog.BackupObject(BackupDialog.BackupObject.Type.FUNCTION,"total_orders")),List.of(new BackupDialog.BackupObject(BackupDialog.BackupObject.Type.EVENT,"daily_cleanup")));
   ((Label)field(dialog,"objectCountLabel")).setText("5");var loading=(Label)field(dialog,"objectLoadingLabel");loading.setVisible(false);loading.setManaged(false);
   var hide=BackupDialog.class.getDeclaredMethod("hideBusyIndicator");hide.setAccessible(true);hide.invoke(dialog);
   var backup=(Button)field(dialog,"backupBtn");backup.setText("备份");backup.setDisable(false);
   var tabs=(TabPane)field(dialog,"tabPane");tabs.getSelectionModel().select(Integer.parseInt(getParameters().getRaw().get(1)));
   if(getParameters().getRaw().size()>2){var select=BackupDialog.class.getDeclaredMethod("selectAllObjects",boolean.class);select.setAccessible(true);select.invoke(dialog,true);}
   var stage=(Stage)field(dialog,"dialogStage");stage.show();stage.setOpacity(.01);
   var wait=new PauseTransition(Duration.seconds(2));wait.setOnFinished(event->{try{
    var root=stage.getScene().getRoot();root.applyCss();root.layout();var path=Path.of(getParameters().getRaw().get(0));
    ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null),null),"png",path.toFile());
    var result=new JsonObject();result.addProperty("width",stage.getScene().getWidth());result.addProperty("height",stage.getScene().getHeight());result.addProperty("loaderDisabled",true);var controls=new JsonArray();walk(root,controls);result.add("controls",controls);
    Files.writeString(Path.of(path+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(result));stage.close();Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
  }
 }
 static void walk(Node node,JsonArray result){
  if(node instanceof Control control&&control.isVisible()){
   var bounds=control.localToScene(control.getLayoutBounds());var row=new JsonObject();row.addProperty("class",control.getClass().getSimpleName());if(control instanceof Labeled label){row.addProperty("text",label.getText());row.addProperty("fontName",label.getFont().getName());row.addProperty("fontSize",label.getFont().getSize());var nativeFont=com.sun.javafx.scene.text.FontHelper.getNativeFont(label.getFont());if(nativeFont instanceof com.sun.javafx.font.PGFont pg){row.addProperty("fontFullName",pg.getFontResource().getFullName());row.addProperty("fontFile",pg.getFontResource().getFileName());}}row.addProperty("x",bounds.getMinX());row.addProperty("y",bounds.getMinY());row.addProperty("width",bounds.getWidth());row.addProperty("height",bounds.getHeight());row.addProperty("disabled",control.isDisabled());result.add(row);
  }
  if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())walk(child,result);
 }
}
