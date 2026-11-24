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
/** Exact original result filter dialog construction; owned scene only. */
public class SqlResultDialogOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage owner)throws Exception {
   owner.setScene(new Scene(new VBox(),900,700));owner.setOpacity(.01);owner.show();
   var dialog=new TextInputDialog("原字段值");dialog.setTitle("自定义筛选");dialog.setHeaderText("输入筛选条件");dialog.setContentText("支持 =、!=、~、!~、<、>：");dialog.initOwner(owner);dialog.show();dialog.getDialogPane().getScene().getWindow().setOpacity(.01);
   var wait=new PauseTransition(Duration.seconds(1));wait.setOnFinished(event->{try{
    var pane=dialog.getDialogPane();pane.applyCss();pane.layout();var path=Path.of(getParameters().getRaw().get(0));ImageIO.write(SwingFXUtils.fromFXImage(pane.snapshot(null,null),null),"png",path.toFile());
    var result=new JsonObject();result.addProperty("width",pane.getWidth());result.addProperty("height",pane.getHeight());var field=dialog.getEditor();var bounds=field.localToScene(field.getLayoutBounds());result.addProperty("inputX",bounds.getMinX());result.addProperty("inputY",bounds.getMinY());result.addProperty("inputWidth",field.getWidth());result.addProperty("inputHeight",field.getHeight());result.addProperty("selectedText",field.getSelectedText());
    var children=new JsonArray();walk(pane,children);result.add("children",children);Files.writeString(Path.of(path+".layout.json"),new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(result));dialog.close();owner.close();Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
  }
 }
 static void walk(Node node,JsonArray output){
  if(node instanceof Labeled label){var b=node.localToScene(node.getLayoutBounds());var item=new JsonObject();item.addProperty("type",node.getClass().getSimpleName());item.addProperty("text",label.getText());item.addProperty("x",b.getMinX());item.addProperty("y",b.getMinY());item.addProperty("width",b.getWidth());item.addProperty("height",b.getHeight());item.addProperty("font",label.getFont().getName());item.addProperty("fontSize",label.getFont().getSize());output.add(item);}
  if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())walk(child,output);
 }
}
