import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.*;
import javafx.stage.*;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import java.nio.file.*;
import javax.imageio.ImageIO;
import com.google.gson.*;
/** The exact TextInputDialog construction used by the original query save handler. */
public class QueryDialogWindowOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage owner)throws Exception {
   owner.setScene(new Scene(new VBox(),900,700));owner.setOpacity(.01);owner.show();
   String mode=System.getProperty("tomato.oracle.mode","save");
   Dialog<?> dialog;
   if(mode.equals("delete")||mode.equals("delete-directory")){
    boolean directory=mode.equals("delete-directory");Alert alert=new Alert(Alert.AlertType.CONFIRMATION);alert.setTitle(directory?"删除目录":"删除查询");alert.setHeaderText(directory?"确定要删除目录 \"原目录\" 及其所有内容吗？":"确定要删除查询 \"原查询\" 吗？");dialog=alert;
   }else{
    TextInputDialog input=new TextInputDialog(mode.equals("directory")?"新目录":mode.equals("rename")?"原查询":"查询1");input.setTitle(mode.equals("directory")?"新建目录":mode.equals("rename")?"重命名查询":"保存查询");input.setHeaderText(null);input.setContentText(mode.equals("directory")?"目录名称：":mode.equals("rename")?"新名称：":"查询名称：");dialog=input;
   }
   dialog.initOwner(owner);dialog.show();dialog.getDialogPane().getScene().getWindow().setOpacity(.01);
   var wait=new PauseTransition(Duration.seconds(1));wait.setOnFinished(event->{try{
    var pane=dialog.getDialogPane();pane.applyCss();pane.layout();var path=Path.of(getParameters().getRaw().get(0));ImageIO.write(SwingFXUtils.fromFXImage(pane.snapshot(null,null),null),"png",path.toFile());
    var result=new JsonObject();result.addProperty("width",pane.getWidth());result.addProperty("height",pane.getHeight());result.addProperty("outerWidth",pane.getScene().getWindow().getWidth());result.addProperty("outerHeight",pane.getScene().getWindow().getHeight());
    if(dialog instanceof TextInputDialog input){var field=input.getEditor();var bounds=field.localToScene(field.getLayoutBounds());result.addProperty("inputX",bounds.getMinX());result.addProperty("inputY",bounds.getMinY());result.addProperty("inputWidth",field.getWidth());result.addProperty("inputHeight",field.getHeight());result.addProperty("selectedText",field.getSelectedText());result.addProperty("font",field.getFont().getName());result.addProperty("fontSize",field.getFont().getSize());}
    var buttons=new JsonArray();for(var type:pane.getButtonTypes()){var button=(Button)pane.lookupButton(type);var rect=button.localToScene(button.getBoundsInLocal());var item=new JsonObject();item.addProperty("text",button.getText());item.addProperty("x",rect.getMinX());item.addProperty("y",rect.getMinY());item.addProperty("width",button.getWidth());item.addProperty("height",button.getHeight());buttons.add(item);}result.add("buttons",buttons);
    Files.writeString(Path.of(path+".layout.json"),new GsonBuilder().setPrettyPrinting().create().toJson(result));dialog.close();owner.close();Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
  }
 }
}
