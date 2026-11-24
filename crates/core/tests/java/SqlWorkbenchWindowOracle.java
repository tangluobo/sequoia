import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.*;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.view.SqlEditorView;
import java.nio.file.*;
import javax.imageio.ImageIO;
/** Current source editor with no configured connection; no network, clipboard or user files. */
public class SqlWorkbenchWindowOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage stage)throws Exception {
   var view=new SqlEditorView(java.util.List.of(),null,null);
   view.setSqlText("SELECT id, name\nFROM Users\nWHERE id > 0;\n-- 中文查询\n");
   stage.setScene(new Scene(view,896,700));stage.show();stage.setOpacity(.01);
   if(getParameters().getRaw().size()>1&&getParameters().getRaw().get(1).equals("pending")){var method=SqlEditorView.class.getDeclaredMethod("setAiSqlGenerating",boolean.class);method.setAccessible(true);method.invoke(view,true);}
   var wait=new PauseTransition(Duration.seconds(2));wait.setOnFinished(event->{try{
    view.applyCss();view.layout();var path=Path.of(getParameters().getRaw().get(0));ImageIO.write(SwingFXUtils.fromFXImage(view.snapshot(null,null),null),"png",path.toFile());
    var result=new JsonObject();result.addProperty("width",view.getWidth());result.addProperty("height",view.getHeight());
    var toolbar=(HBox)view.getTop();result.addProperty("toolbarHeight",toolbar.getHeight());var controls=new JsonArray();
    for(var node:toolbar.getChildren()){var item=new JsonObject();var rect=node.localToScene(node.getLayoutBounds());item.addProperty("class",node.getClass().getSimpleName());if(node instanceof Labeled label)item.addProperty("text",label.getText());item.addProperty("x",rect.getMinX());item.addProperty("y",rect.getMinY());item.addProperty("width",rect.getWidth());item.addProperty("height",rect.getHeight());controls.add(item);}result.add("controls",controls);
    var split=(SplitPane)view.getCenter();result.addProperty("divider",split.getDividerPositions()[0]);
    var textNodes=new JsonArray();walk(view,textNodes);result.add("textNodes",textNodes);
    Files.writeString(Path.of(path+".layout.json"),new GsonBuilder().setPrettyPrinting().create().toJson(result));stage.close();Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
  }
 }
 static void walk(Node node,JsonArray rows){
  if(rows.size()>=100)return;
  if(node instanceof javafx.scene.text.Text text&&!text.getText().isEmpty()){
   var item=new JsonObject();var rect=text.localToScene(text.getLayoutBounds());item.addProperty("text",text.getText());item.addProperty("fill",text.getFill().toString());item.addProperty("font",text.getFont().getName());item.addProperty("size",text.getFont().getSize());item.addProperty("x",rect.getMinX());item.addProperty("y",rect.getMinY());item.addProperty("width",rect.getWidth());item.addProperty("height",rect.getHeight());rows.add(item);
  }
  if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())walk(child,rows);
 }
}
