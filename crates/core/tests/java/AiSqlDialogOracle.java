import javafx.application.*;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.*;
import javafx.stage.*;
import javafx.embed.swing.SwingFXUtils;
import javax.imageio.ImageIO;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.dialog.*;
import java.nio.file.*;
/** Captures only scenes created by this probe. Cancels all dialogs without saving. */
public class AiSqlDialogOracle {
 public static void main(String[] args){Platform.startup(()->Platform.runLater(()->{try{
  var owner=new StackPane();var stage=new Stage();stage.setScene(new Scene(owner,800,600));stage.show();
  for(String kind:new String[]{"prompt","settings","settings-valid"}){
   Runnable capture=()->{try{
    var window=Window.getWindows().stream().filter(w->w!=stage&&w.isShowing()).findFirst().orElseThrow();var scene=window.getScene();scene.getRoot().applyCss();scene.getRoot().layout();
    var out=new JsonObject();out.addProperty("focusOwner",scene.getFocusOwner()==null?"":scene.getFocusOwner().getClass().getSimpleName());out.addProperty("width",scene.getWidth());out.addProperty("height",scene.getHeight());var nodes=new JsonArray();collect(scene.getRoot(),nodes);out.add("nodes",nodes);
    Files.writeString(Path.of(args[0]+"-"+kind+".json"),new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(out));ImageIO.write(SwingFXUtils.fromFXImage(scene.snapshot(null),null),"png",Path.of(args[0]+"-"+kind+".png").toFile());
    scene.getRoot().lookupAll(".button").stream().filter(n->n instanceof Button b&&b.isCancelButton()).map(n->(Button)n).findFirst().orElseThrow().fire();
   }catch(Throwable error){error.printStackTrace();System.exit(1);}};Thread.ofVirtual().start(()->{try{Thread.sleep(700);}catch(InterruptedException error){Thread.currentThread().interrupt();return;}Platform.runLater(capture);});
   if(kind.equals("prompt"))AiSqlPromptDialog.show(owner,"");else {if(kind.equals("settings-valid"))com.tangluobo.tomato.module.connect.GlobalConfig.getInstance().setAiModel("fixture-model");AiSqlConfigDialog.show(owner);}
  }stage.close();Platform.exit();System.exit(0);
 }catch(Throwable error){error.printStackTrace();System.exit(1);}}));}
 static void collect(Node node,JsonArray nodes){var bounds=node.localToScene(node.getBoundsInLocal());var item=new JsonObject();item.addProperty("focused",node.isFocused());item.addProperty("disabled",node.isDisabled());item.addProperty("class",node.getClass().getSimpleName());item.addProperty("text",node instanceof Labeled l?l.getText():node instanceof TextInputControl t?t.getText():"");item.addProperty("x",bounds.getMinX());item.addProperty("y",bounds.getMinY());item.addProperty("w",bounds.getWidth());item.addProperty("h",bounds.getHeight());nodes.add(item);if(node instanceof Parent p)for(var child:p.getChildrenUnmodifiable())collect(child,nodes);}
}
