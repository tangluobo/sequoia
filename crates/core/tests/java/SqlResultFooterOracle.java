import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.*;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import javafx.collections.FXCollections;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.TableRowData;
import com.tangluobo.tomato.module.connect.view.SqlEditorView;
import java.nio.file.*;
import javax.imageio.ImageIO;
/** Own original read-only result/footer scene; no network or clipboard access. */
public class SqlResultFooterOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage stage)throws Exception {
   var view=new SqlEditorView(java.util.List.of(),null,null);var result=new TableRowData();result.setColumnNames(java.util.List.of("missing","empty","literal","中文字段"));
   for(int i=0;i<2000;i++)result.getRows().add(FXCollections.observableArrayList(java.util.Arrays.asList(null,"","NULL","第 "+i+" 行")));
   var method=java.util.Arrays.stream(SqlEditorView.class.getDeclaredMethods()).filter(candidate->candidate.getName().equals("createTableView")&&candidate.getParameterCount()==5).findFirst().orElseThrow();method.setAccessible(true);
   var node=(Node)method.invoke(view,result,null,null,null,true);var root=new StackPane(node);root.getStylesheets().addAll(view.getStylesheets());stage.setScene(new Scene(root,700,450));stage.setOpacity(.01);stage.show();
   var wait=new PauseTransition(Duration.seconds(2));wait.setOnFinished(event->{try {
    root.applyCss();root.layout();var path=Path.of(getParameters().getRaw().get(0));ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null),null),"png",path.toFile());
    var output=new JsonObject();var tables=new JsonArray();var texts=new JsonArray();SqlResultWindowOracle.walk(root,tables,texts);output.add("tables",tables);output.add("texts",texts);var buttons=new JsonArray();for(var candidate:root.lookupAll(".toolbar-button")){if(candidate instanceof Button button){var item=rect(button);item.addProperty("accessible",button.getAccessibleText());item.addProperty("disabled",button.isDisabled());item.addProperty("opacity",button.getOpacity());if(button.getGraphic() instanceof javafx.scene.shape.SVGPath svg){item.add("graphic",rect(svg));item.addProperty("path",svg.getContent());item.addProperty("stroke",svg.getStrokeWidth());item.addProperty("lineCap",svg.getStrokeLineCap().toString());}buttons.add(item);}}output.add("buttons",buttons);
    var boxes=new JsonArray();for(var candidate:((BorderPane)node).getChildren()){if(candidate instanceof VBox box)for(var child:box.getChildren()){var item=rect(child);item.addProperty("type",child.getClass().getSimpleName());boxes.add(item);}}output.add("footer",boxes);
    Files.writeString(Path.of(path+".layout.json"),new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(output));stage.close();Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
  }
 }
 static JsonObject rect(Node node){var bounds=node.localToScene(node.getLayoutBounds());var item=new JsonObject();item.addProperty("x",bounds.getMinX());item.addProperty("y",bounds.getMinY());item.addProperty("width",bounds.getWidth());item.addProperty("height",bounds.getHeight());return item;}
}
