import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.stage.*;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.dialog.RestoreDialog;
import com.tangluobo.tomato.module.connect.ConnectionConfig;
import com.tangluobo.tomato.module.connect.ConnectType;
import java.nio.file.*;
import javax.imageio.ImageIO;
/** Captures the actual original confirmation and fires NO before any restore task exists. */
public class RestoreConfirmationOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  JsonObject result;Stage original;RestoreDialog dialog;Path output;
  public void start(Stage unused)throws Exception {
   output=Path.of(getParameters().getRaw().get(0));var config=new ConnectionConfig();config.setType(ConnectType.MYSQL);config.setName("本地数据库");
   dialog=new RestoreDialog(null,config,"sample_db","20261018120000","");
   var field=RestoreDialog.class.getDeclaredField("dialogStage");field.setAccessible(true);original=(Stage)field.get(dialog);original.show();original.setOpacity(.01);
   var inspect=new PauseTransition(Duration.millis(400));var deadline=System.nanoTime()+15_000_000_000L;
   inspect.setOnFinished(event->{try{
    for(var window:java.util.List.copyOf(Window.getWindows())){
     if(window==original||window.getScene()==null||!(window.getScene().getRoot() instanceof DialogPane pane))continue;
     window.setOpacity(.01);pane.applyCss();pane.layout();ImageIO.write(SwingFXUtils.fromFXImage(pane.snapshot(null,null),null),"png",output.toFile());
     var parameters=new SnapshotParameters();parameters.setFill(javafx.scene.paint.Color.TRANSPARENT);ImageIO.write(SwingFXUtils.fromFXImage(pane.getGraphic().snapshot(parameters,null),null),"png",output.resolveSibling("dialog-warning-103.png").toFile());
     result=new JsonObject();result.addProperty("width",window.getScene().getWidth());result.addProperty("height",window.getScene().getHeight());result.addProperty("header",pane.getHeaderText());result.addProperty("content",pane.getContentText());var controls=new JsonArray();walk(pane,controls);result.add("controls",controls);
     ((Button)pane.lookupButton(ButtonType.NO)).fire();return;
    }
    if(System.nanoTime()>deadline)throw new IllegalStateException("Owned confirmation did not appear");inspect.playFromStart();
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});inspect.play();
   Platform.runLater(()->{try{
    var method=RestoreDialog.class.getDeclaredMethod("startRestore",Button.class);method.setAccessible(true);method.invoke(dialog,new Button());
    var task=RestoreDialog.class.getDeclaredField("currentTask");task.setAccessible(true);boolean requested=task.get(dialog)!=null;
    if(result==null||requested)throw new IllegalStateException("Confirmation must decline before creating a database task");
    result.addProperty("databaseRequested",requested);Files.writeString(Path.of(output+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(result));original.close();Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});
  }
 }
 static void walk(Node node,JsonArray result){
  if(node instanceof Control control&&control.isVisible()){
   var b=control.localToScene(control.getLayoutBounds());var row=new JsonObject();row.addProperty("class",control.getClass().getSimpleName());if(control instanceof Labeled label){row.addProperty("text",label.getText());row.addProperty("fontSize",label.getFont().getSize());}row.addProperty("x",b.getMinX());row.addProperty("y",b.getMinY());row.addProperty("width",b.getWidth());row.addProperty("height",b.getHeight());result.add(row);
  }
  if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())walk(child,result);
 }
}
