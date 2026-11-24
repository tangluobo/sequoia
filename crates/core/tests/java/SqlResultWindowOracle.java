import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.*;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.collections.FXCollections;
import javafx.embed.swing.SwingFXUtils;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.TableRowData;
import com.tangluobo.tomato.module.connect.view.SqlEditorView;
import java.nio.file.*;
import javax.imageio.ImageIO;
/** Original read-only result widget, owned scene only; no connection or clipboard. */
public class SqlResultWindowOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage stage)throws Exception {
   var view=new SqlEditorView(java.util.List.of(),null,null);
   var result=new TableRowData();result.setColumnNames(java.util.List.of("missing","empty","literal","中文字段"));
   for(int i=0;i<2000;i++)result.getRows().add(FXCollections.observableArrayList(java.util.Arrays.asList(null,"","NULL","第 "+i+" 行")));
   var method=SqlEditorView.class.getDeclaredMethod("createTableView",TableRowData.class);method.setAccessible(true);
   var node=(Node)method.invoke(view,result);var root=new StackPane(node);root.getStylesheets().addAll(view.getStylesheets());
   stage.setScene(new Scene(root,700,450));stage.setOpacity(.01);stage.show();
   var wait=new PauseTransition(Duration.seconds(2));wait.setOnFinished(event->{try{
    root.applyCss();root.layout();var path=Path.of(getParameters().getRaw().get(0));ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null),null),"png",path.toFile());
    var info=new JsonObject();var tables=new JsonArray();var texts=new JsonArray();walk(root,tables,texts);info.add("tables",tables);info.add("texts",texts);var bars=new JsonArray();for(var candidate:root.lookupAll(".scroll-bar")){if(candidate instanceof ScrollBar bar&&bar.isVisible()){var bounds=bar.localToScene(bar.getLayoutBounds());if(bounds.getWidth()<=0||bounds.getHeight()<=0)continue;var item=new JsonObject();item.addProperty("orientation",bar.getOrientation().toString());item.addProperty("x",bounds.getMinX());item.addProperty("y",bounds.getMinY());item.addProperty("width",bounds.getWidth());item.addProperty("height",bounds.getHeight());var before=bar.getValue();item.addProperty("max",bar.getMax());item.addProperty("visibleAmount",bar.getVisibleAmount());bar.increment();item.addProperty("incrementDelta",bar.getValue()-before);bar.setValue(before);bar.adjustValue(1.0);item.addProperty("pageDelta",bar.getValue()-before);bar.setValue(before);bars.add(item);}}info.add("scrollbars",bars);
    Files.writeString(Path.of(path+".layout.json"),new GsonBuilder().setPrettyPrinting().create().toJson(info));stage.close();Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
  }
 }
 static void walk(Node node,JsonArray tables,JsonArray texts){
  if(node instanceof TableView<?> table){var item=new JsonObject();item.addProperty("fixedRowHeight",table.getFixedCellSize());item.addProperty("rows",table.getItems().size());var columns=new JsonArray();for(var col:table.getColumns()){var value=new JsonObject();value.addProperty("text",col.getText());value.addProperty("width",col.getWidth());columns.add(value);}item.add("columns",columns);tables.add(item);}
  if(node instanceof javafx.scene.text.Text text&&!text.getText().isEmpty()&&texts.size()<90){var item=new JsonObject();item.addProperty("text",text.getText());item.addProperty("font",text.getFont().getName());item.addProperty("size",text.getFont().getSize());var nativeFont=(com.sun.javafx.font.PGFont)com.sun.javafx.scene.text.FontHelper.getNativeFont(text.getFont());item.addProperty("nativeFont",nativeFont.getFontResource().getFullName());item.addProperty("fontFile",nativeFont.getFontResource().getFileName());item.addProperty("fill",text.getFill().toString());var rect=text.localToScene(text.getLayoutBounds());item.addProperty("x",rect.getMinX());item.addProperty("y",rect.getMinY());item.addProperty("width",rect.getWidth());item.addProperty("height",rect.getHeight());texts.add(item);}
  if(node instanceof Parent parent)for(var child:parent.getChildrenUnmodifiable())walk(child,tables,texts);
 }
}
