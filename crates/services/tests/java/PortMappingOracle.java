import javafx.application.*;
import javafx.stage.*;
import javafx.scene.*;
import javafx.scene.layout.*;
import javafx.scene.control.*;
import javafx.animation.*;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import javax.imageio.ImageIO;
import java.nio.file.*;
import java.util.*;
import com.google.gson.*;
import com.tangluobo.tomato.module.tools.PortMappingPane;
import com.tangluobo.tomato.module.connect.*;
/** Actual original source, isolated home/preferences; only own scenes, no SSH operations. */
public class PortMappingOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 static void save(Scene scene,Path image)throws Exception {
  scene.getRoot().applyCss();scene.getRoot().layout();ImageIO.write(SwingFXUtils.fromFXImage(scene.snapshot(null),null),"png",image.toFile());
  JsonArray nodes=new JsonArray();ToolPaneOracle.nodes(scene.getRoot(),nodes);Files.writeString(Path.of(image.toString()+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(nodes));
 }
 public static class Probe extends Application {
  public void start(Stage stage)throws Exception {
   Path out=Path.of(getParameters().getRaw().get(0));Files.createDirectories(out);
   if(!System.getProperty("user.home").contains("oracle-port-"))throw new IllegalStateException("Requires isolated home");
   ConnectionConfig host=new ConnectionConfig();host.setId("owned-ssh");host.setName("隔离 SSH 主机");host.setType(ConnectType.SSH);host.setHost("127.0.0.1");host.setPort(22);ConfigManager.saveConnections(List.of(host));
   PortMappingPane pane=new PortMappingPane();pane.getChildren().removeFirst();stage.setScene(new Scene(pane,896,650));stage.setOpacity(.01);stage.show();stage.toBack();
   PauseTransition first=new PauseTransition(Duration.millis(300));first.setOnFinished(e->Platform.runLater(()->{try{
    save(stage.getScene(),out.resolve("table.png"));
    PauseTransition second=new PauseTransition(Duration.millis(300));second.setOnFinished(event->{try{
     for(Window w:List.copyOf(Window.getWindows()))if(w instanceof Stage child&&child!=stage&&w.isShowing()){
      child.setOpacity(.01);child.toBack();save(child.getScene(),out.resolve("dialog.png"));child.close();
     }
    }catch(Exception ex){ex.printStackTrace();System.exit(1);}});second.play();
    var field=PortMappingPane.class.getDeclaredField("addBtn");field.setAccessible(true);((Button)field.get(pane)).fire();stage.close();Platform.exit();
   }catch(Exception ex){ex.printStackTrace();System.exit(1);}}));first.play();
  }
 }
}
