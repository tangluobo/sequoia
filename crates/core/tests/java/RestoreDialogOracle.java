import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.dialog.RestoreDialog;
import com.tangluobo.tomato.module.connect.ConnectionConfig;
import com.tangluobo.tomato.module.connect.ConnectType;
import java.nio.file.*;
import javax.imageio.ImageIO;
/** Original restore UI only. No action is fired and no database is opened. */
public class RestoreDialogOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage owner)throws Exception {
   var config=new ConnectionConfig();config.setType(ConnectType.MYSQL);config.setName("本地数据库");
   var dialog=new RestoreDialog(null,config,"sample_db","20261018120000","");
   var field=RestoreDialog.class.getDeclaredField("dialogStage");field.setAccessible(true);var stage=(Stage)field.get(dialog);
   stage.show();stage.setOpacity(.01);
   var wait=new PauseTransition(Duration.seconds(2));wait.setOnFinished(event->{try{
    var root=stage.getScene().getRoot();root.applyCss();root.layout();var path=Path.of(getParameters().getRaw().get(0));
    ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null),null),"png",path.toFile());
    var result=new JsonObject();result.addProperty("width",stage.getScene().getWidth());result.addProperty("height",stage.getScene().getHeight());var controls=new JsonArray();walk(root,controls);result.add("controls",controls);
    Files.writeString(Path.of(path+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(result));stage.close();Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
  }
 }
 static void walk(Node node,JsonArray result){
  if(node instanceof Control control&&control.isVisible()){
   var bounds=control.localToScene(control.getLayoutBounds());var row=new JsonObject();row.addProperty("class",control.getClass().getSimpleName());if(control instanceof Labeled label)row.addProperty("text",label.getText());row.addProperty("x",bounds.getMinX());row.addProperty("y",bounds.getMinY());row.addProperty("width",bounds.getWidth());row.addProperty("height",bounds.getHeight());row.addProperty("disabled",control.isDisabled());result.add(row);
  }
  if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())walk(child,result);
 }
}
