import javafx.application.*;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.stage.*;
import javafx.embed.swing.SwingFXUtils;
import javax.imageio.ImageIO;
import java.nio.file.*;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.view.SqlEditorPane;
/** Only shows and snapshots its own menu; does not activate clipboard actions. */
public class SqlEditorMenuOracle {
 public static void main(String[] args){Platform.startup(()->{try{
  var pane=new SqlEditorPane();pane.setText("SELECT 1;\n-- ai: 人数");pane.selectRange(0,8);pane.setContextMenuActions(()->{},()->{});
  var stage=new Stage();var scene=new Scene(pane,800,600);scene.getStylesheets().add(SqlEditorPane.class.getResource("/css/connect-tree.css").toExternalForm());stage.setScene(scene);stage.show();
  var field=SqlEditorPane.class.getDeclaredField("editorContextMenu");field.setAccessible(true);var menu=(ContextMenu)field.get(pane);menu.show(stage,stage.getX()+40,stage.getY()+60);
  Platform.runLater(()->{try{var menuScene=menu.getScene();menuScene.getRoot().applyCss();menuScene.getRoot().layout();var out=new JsonObject();out.addProperty("width",menuScene.getWidth());out.addProperty("height",menuScene.getHeight());var nodes=new JsonArray();AiSqlDialogOracle.collect(menuScene.getRoot(),nodes);out.add("nodes",nodes);Files.writeString(Path.of(args[0]+".json"),new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(out));ImageIO.write(SwingFXUtils.fromFXImage(menuScene.snapshot(null),null),"png",Path.of(args[0]+".png").toFile());menu.hide();stage.close();Platform.exit();System.exit(0);}catch(Throwable error){error.printStackTrace();System.exit(1);}});
 }catch(Throwable error){error.printStackTrace();System.exit(1);}});}
}
