import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.Pane;
import javafx.stage.*;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import java.nio.file.*;
import com.google.gson.*;
import javax.imageio.ImageIO;
import com.tangluobo.tomato.module.connect.dialog.GlobalConfigDialog;

/** Original source in an owned JavaFX component fixture. Cancels without saving. */
public class TableConfigOracle {
 public static void main(String[] args) { Application.launch(Probe.class,args); }
 static void record(Node node, JsonArray nodes) {
  var item=new JsonObject();item.addProperty("class",node.getClass().getSimpleName());
  if(node instanceof Labeled label)item.addProperty("text",label.getText());
  if(node instanceof ComboBox<?> box) {item.addProperty("value",String.valueOf(box.getValue()));var values=new JsonArray();for(var value:box.getItems())values.add(String.valueOf(value));item.add("items",values);}
  if(node instanceof Spinner<?> spinner)item.addProperty("value",String.valueOf(spinner.getValue()));
  var b=node.localToScene(node.getBoundsInLocal());item.addProperty("x",b.getMinX());item.addProperty("y",b.getMinY());item.addProperty("width",b.getWidth());item.addProperty("height",b.getHeight());nodes.add(item);
  if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())record(child,nodes);
 }
 public static class Probe extends Application {
  public void start(Stage owner) {
   var output=Path.of(getParameters().getRaw().get(0));
   owner.setScene(new Scene(new Pane(),20,20));owner.setOpacity(.01);owner.show();
   var timer=new PauseTransition(Duration.seconds(2));timer.setOnFinished(event->{
    try {
     var window=Window.getWindows().stream().filter(w->w!=owner&&w.isShowing()&&w.getScene().getRoot() instanceof DialogPane).findFirst().orElseThrow();
     var pane=(DialogPane)window.getScene().getRoot();pane.applyCss();pane.layout();
     ImageIO.write(SwingFXUtils.fromFXImage(pane.snapshot(null,null),null),"png",output.toFile());
     var nodes=new JsonArray();record(pane,nodes);Files.writeString(Path.of(output+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(nodes));
     var cancel=pane.getButtonTypes().stream().filter(b->b.getText().equals("取消")).findFirst().orElseThrow();((ButtonBase)pane.lookupButton(cancel)).fire();
    }catch(Exception error){error.printStackTrace();System.exit(2);}
   });timer.play();
   GlobalConfigDialog.show(owner,GlobalConfigDialog.ConfigMode.TABLE);
   owner.close();Platform.exit();System.exit(0);
  }
 }
}
